"""Integration tests for the :class:`Gateway` proxy.

Spins up a local ``aiohttp`` server that imitates Anthropic / OpenAI
response shapes and verifies that the proxy:

* forwards the request body + ``Authorization`` header verbatim
* streams SSE bytes back to the client without reordering
* counts request / response bytes accurately
* accumulates vendor usage from both SSE and JSON responses
* returns ``404`` for unknown sessions and ``502`` when the upstream is
  unreachable
"""

from __future__ import annotations

import asyncio
import json
import socket
import threading
import urllib.error
import urllib.request
from contextlib import contextmanager
from typing import Iterator

import pytest
from aiohttp import web

from bnf.proxy import Gateway


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class _Upstream:
    """Minimal fake vendor server running in its own thread."""

    def __init__(self, app: web.Application) -> None:
        self._app = app
        self._loop: asyncio.AbstractEventLoop | None = None
        self._runner: web.AppRunner | None = None
        self._site: web.TCPSite | None = None
        self._thread: threading.Thread | None = None
        self._ready = threading.Event()
        self._port = 0

    @property
    def base_url(self) -> str:
        return f"http://127.0.0.1:{self._port}"

    def start(self) -> None:
        self._port = _free_port()
        self._thread = threading.Thread(target=self._run, daemon=True, name="bnf-upstream")
        self._thread.start()
        self._ready.wait(timeout=10)

    def _run(self) -> None:
        loop = asyncio.new_event_loop()
        self._loop = loop
        asyncio.set_event_loop(loop)
        loop.run_until_complete(self._setup())
        self._ready.set()
        try:
            loop.run_forever()
        finally:
            loop.run_until_complete(self._teardown())
            loop.close()

    async def _setup(self) -> None:
        self._runner = web.AppRunner(self._app, access_log=None)
        await self._runner.setup()
        self._site = web.TCPSite(self._runner, "127.0.0.1", self._port)
        await self._site.start()

    async def _teardown(self) -> None:
        if self._site is not None:
            await self._site.stop()
        if self._runner is not None:
            await self._runner.cleanup()

    def stop(self) -> None:
        if self._loop is None:
            return
        self._loop.call_soon_threadsafe(self._loop.stop)
        if self._thread is not None:
            self._thread.join(timeout=5)


@contextmanager
def _upstream(app: web.Application) -> Iterator[_Upstream]:
    srv = _Upstream(app)
    srv.start()
    try:
        yield srv
    finally:
        srv.stop()


@contextmanager
def _gateway() -> Iterator[Gateway]:
    gw = Gateway.start()
    try:
        yield gw
    finally:
        gw.stop()


def _http_post(
    url: str,
    body: bytes,
    headers: dict[str, str] | None = None,
    method: str = "POST",
) -> tuple[int, dict[str, str], bytes]:
    req = urllib.request.Request(url, data=body, method=method)
    for k, v in (headers or {}).items():
        req.add_header(k, v)
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            return resp.status, dict(resp.headers), resp.read()
    except urllib.error.HTTPError as exc:
        return exc.code, dict(exc.headers or {}), exc.read()


def _http_stream(
    url: str,
    body: bytes,
    headers: dict[str, str] | None = None,
) -> tuple[int, dict[str, str], list[bytes]]:
    req = urllib.request.Request(url, data=body, method="POST")
    for k, v in (headers or {}).items():
        req.add_header(k, v)
    chunks: list[bytes] = []
    with urllib.request.urlopen(req, timeout=10) as resp:
        while True:
            chunk = resp.read(4096)
            if not chunk:
                break
            chunks.append(chunk)
        return resp.status, dict(resp.headers), chunks


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_rejects_unknown_session() -> None:
    with _gateway() as gw:
        status, _headers, body = _http_post(
            f"{gw.base_url}/s/does-not-exist/v1/messages",
            b"{}",
            {"Content-Type": "application/json"},
        )
    assert status == 404
    payload = json.loads(body)
    assert payload["error"]["type"] == "proxy_error"


def test_forwards_json_request_and_tracks_bytes() -> None:
    captured: dict[str, object] = {}

    async def handler(request: web.Request) -> web.Response:
        captured["path"] = request.path
        captured["method"] = request.method
        captured["auth"] = request.headers.get("Authorization")
        captured["api_key"] = request.headers.get("x-api-key")
        captured["body"] = await request.read()
        return web.json_response(
            {
                "id": "msg_1",
                "usage": {
                    "input_tokens": 42,
                    "output_tokens": 7,
                    "cache_read_input_tokens": 3,
                },
            }
        )

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        req_body = b'{"model":"claude-sonnet-4-6","messages":[]}'
        status, _headers, resp_body = _http_post(
            f"{session.base_url}/v1/messages",
            req_body,
            {
                "Content-Type": "application/json",
                "Authorization": "Bearer abc123",
                "x-api-key": "sk-test",
            },
        )
        assert status == 200
        assert json.loads(resp_body)["id"] == "msg_1"
        stats = gw.get_stats(session.id)

    assert captured["path"] == "/v1/messages"
    assert captured["method"] == "POST"
    assert captured["auth"] == "Bearer abc123"
    assert captured["api_key"] == "sk-test"
    assert captured["body"] == req_body

    assert stats.requests == 1
    assert stats.raw_request_bytes == len(req_body)
    assert stats.raw_response_bytes > 0
    # Anthropic: input=42 + cache_read=3, no cache_creation → total=45, uncached=42, cached=3.
    assert stats.vendor.total_input_tokens == 45
    assert stats.vendor.cached_input_tokens == 3
    assert stats.vendor.uncached_input_tokens == 42
    assert stats.vendor.non_reasoning_output_tokens == 7
    assert stats.vendor.reasoning_output_tokens == 0
    assert stats.vendor.total_tokens == 45 + 7
    assert len(stats.events) == 1
    evt = stats.events[0]
    assert evt.status == 200
    assert evt.streaming is False
    assert evt.usage is not None
    # Old vendor-specific field name should NOT appear in event.usage.
    assert "input_tokens" not in evt.usage
    assert "output_tokens" not in evt.usage
    assert evt.usage["total_input_tokens"] == 45


def test_streams_sse_and_accumulates_usage() -> None:
    async def handler(request: web.Request) -> web.StreamResponse:
        assert request.method == "POST"
        resp = web.StreamResponse(
            status=200,
            headers={"Content-Type": "text/event-stream"},
        )
        resp.enable_chunked_encoding()
        await resp.prepare(request)
        events = [
            {
                "type": "message_start",
                "message": {
                    "id": "msg_1",
                    "usage": {
                        "input_tokens": 100,
                        "cache_read_input_tokens": 20,
                        "output_tokens": 1,
                    },
                },
            },
            {
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""},
            },
            {
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "hello"},
            },
            {"type": "content_block_stop", "index": 0},
            {
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 55},
            },
            {"type": "message_stop"},
        ]
        for ev in events:
            payload = (
                f"event: {ev['type']}\n".encode("utf-8")
                + b"data: "
                + json.dumps(ev).encode("utf-8")
                + b"\n\n"
            )
            await resp.write(payload)
        await resp.write_eof()
        return resp

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        status, headers, chunks = _http_stream(
            f"{session.base_url}/v1/messages",
            b'{"stream":true}',
            {"Content-Type": "application/json"},
        )
        stats = gw.get_stats(session.id)

    assert status == 200
    assert "text/event-stream" in headers.get("Content-Type", "")
    body = b"".join(chunks)
    assert b"message_start" in body
    assert b"message_delta" in body

    assert stats.requests == 1
    assert stats.vendor.uncached_input_tokens == 100
    assert stats.vendor.cached_input_tokens == 20
    assert stats.vendor.total_input_tokens == 120
    assert stats.vendor.non_reasoning_output_tokens == 55
    assert stats.vendor.total_tokens == 120 + 55
    assert len(stats.events) == 1
    evt = stats.events[0]
    assert evt.streaming is True
    assert evt.raw_response_bytes == len(body)
    # Content analyzer should have picked up "hello" from the text_delta.
    assert stats.content_from_model.assistant_text_bytes == len(b"hello")


def test_openai_responses_json_body() -> None:
    async def handler(request: web.Request) -> web.Response:
        return web.json_response(
            {
                "id": "resp_1",
                "usage": {
                    "input_tokens": 80,
                    "input_tokens_details": {"cached_tokens": 12},
                    "output_tokens": 33,
                    "output_tokens_details": {"reasoning_tokens": 11},
                    "total_tokens": 113,
                },
            }
        )

    app = web.Application()
    app.router.add_post("/v1/responses", handler)

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="openai", upstream_base=srv.base_url)
        _http_post(
            f"{session.base_url}/v1/responses",
            b"{}",
            {"Content-Type": "application/json"},
        )
        stats = gw.get_stats(session.id)

    assert stats.vendor.total_input_tokens == 80
    assert stats.vendor.cached_input_tokens == 12
    assert stats.vendor.uncached_input_tokens == 80 - 12
    assert stats.vendor.reasoning_output_tokens == 11
    assert stats.vendor.non_reasoning_output_tokens == 33 - 11
    assert stats.vendor.total_tokens == 113


def test_multiple_requests_accumulate() -> None:
    call_count = 0

    async def handler(request: web.Request) -> web.Response:
        nonlocal call_count
        call_count += 1
        return web.json_response({"usage": {"input_tokens": 10, "output_tokens": 2}})

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        for _ in range(3):
            _http_post(
                f"{session.base_url}/v1/messages",
                b"{}",
                {"Content-Type": "application/json"},
            )
        stats = gw.get_stats(session.id)

    assert call_count == 3
    assert stats.requests == 3
    # Three 10-input responses → uncached=30, cached=0, total_input=30.
    assert stats.vendor.uncached_input_tokens == 30
    assert stats.vendor.cached_input_tokens == 0
    assert stats.vendor.total_input_tokens == 30
    assert stats.vendor.non_reasoning_output_tokens == 6
    assert stats.vendor.total_tokens == 3 * (10 + 2)
    assert len(stats.events) == 3


def test_upstream_unreachable_returns_502() -> None:
    with _gateway() as gw:
        # Point at a closed port.
        dead_port = _free_port()
        session = gw.create_session(
            upstream="anthropic",
            upstream_base=f"http://127.0.0.1:{dead_port}",
        )
        status, _headers, body = _http_post(
            f"{session.base_url}/v1/messages",
            b"{}",
            {"Content-Type": "application/json"},
        )
        stats = gw.get_stats(session.id)

    assert status == 502
    payload = json.loads(body)
    assert payload["error"]["type"] == "proxy_upstream_error"
    assert stats.requests == 1
    assert len(stats.events) == 1
    assert stats.events[0].error is not None


def test_query_string_and_path_are_forwarded() -> None:
    captured: dict[str, str] = {}

    async def handler(request: web.Request) -> web.Response:
        captured["path"] = request.path
        captured["query"] = request.query_string
        return web.json_response({"ok": True})

    app = web.Application()
    app.router.add_get("/v1/models/list", handler)

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        url = f"{session.base_url}/v1/models/list?limit=5&order=asc"
        req = urllib.request.Request(url, method="GET")
        with urllib.request.urlopen(req, timeout=10) as resp:
            assert resp.status == 200

    assert captured["path"] == "/v1/models/list"
    assert captured["query"] == "limit=5&order=asc"


def test_sniffs_sse_when_content_type_missing() -> None:
    """The ChatGPT backend omits Content-Type on /responses streams —
    the proxy must detect SSE from the first chunk's ``event:``/``data:`` prefix."""

    async def handler(request: web.Request) -> web.StreamResponse:
        # Deliberately no Content-Type header.
        resp = web.StreamResponse(status=200)
        resp.enable_chunked_encoding()
        await resp.prepare(request)
        events = [
            {
                "type": "response.created",
                "response": {"id": "resp_1"},
            },
            {
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "usage": {
                        "input_tokens": 40,
                        "output_tokens": 9,
                        "total_tokens": 49,
                    },
                },
            },
        ]
        for ev in events:
            await resp.write(
                f"event: {ev['type']}\n".encode("utf-8")
                + b"data: "
                + json.dumps(ev).encode("utf-8")
                + b"\n\n"
            )
        await resp.write_eof()
        return resp

    app = web.Application()
    app.router.add_post("/responses", handler)

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="chatgpt", upstream_base=srv.base_url)
        _http_post(
            f"{session.base_url}/responses",
            b"{}",
            {"Content-Type": "application/json"},
        )
        stats = gw.get_stats(session.id)

    assert stats.vendor.total_tokens == 49
    assert len(stats.events) == 1
    # streaming flag should have been flipped on during sniffing.
    assert stats.events[0].streaming is True


def test_close_session_returns_final_stats_and_removes_it() -> None:
    async def handler(request: web.Request) -> web.Response:
        return web.json_response({"usage": {"input_tokens": 1, "output_tokens": 1}})

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        _http_post(
            f"{session.base_url}/v1/messages",
            b"{}",
            {"Content-Type": "application/json"},
        )
        final = gw.close_session(session.id)
        with pytest.raises(KeyError):
            gw.get_stats(session.id)

    assert final.requests == 1
    assert final.vendor.uncached_input_tokens == 1
    assert final.vendor.total_input_tokens == 1


def test_three_way_split_claude_like_pattern() -> None:
    """Claude's five-event shape: pre-flight text → main bootstrap w/ first
    tool_use → subagent w/ tool_uses → subagent summary → final answer.

    Expected split:
      * bootstrap = [#0]      (pre-flight, response is text only)
      * driven    = [#1..#3]  (loop body — excludes final)
      * final     = [#4]      (last event)
    """

    call_n = 0

    async def handler(request: web.Request) -> web.Response:
        nonlocal call_n
        call_n += 1
        if call_n == 1:
            content = [{"type": "text", "text": "ok pre-flight"}]
        elif call_n == 2:
            content = [
                {"type": "text", "text": "let me delegate"},
                {
                    "type": "tool_use",
                    "id": "t1",
                    "name": "Agent",
                    "input": {"prompt": "read files"},
                },
            ]
        elif call_n == 3:
            content = [
                {
                    "type": "tool_use",
                    "id": "t2",
                    "name": "Read",
                    "input": {"path": "a.py"},
                }
            ]
        elif call_n == 4:
            content = [{"type": "text", "text": "subagent summary"}]
        else:  # call_n == 5
            content = [{"type": "text", "text": "final answer"}]
        return web.json_response(
            {
                "content": content,
                "usage": {"input_tokens": 10 * call_n, "output_tokens": call_n},
            }
        )

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        for _ in range(5):
            _http_post(
                f"{session.base_url}/v1/messages",
                b'{"messages":[{"role":"user","content":"hi"}]}',
                {"Content-Type": "application/json"},
            )
        stats = gw.get_stats(session.id)

    assert stats.loop_start_index == 1  # event #1 is the first with tool_call in resp
    assert stats.bootstrap.event_count == 1
    assert stats.driven.event_count == 3
    assert stats.final.event_count == 1

    # Tokens sum cleanly across the three slices: 1+2+3+4+5 calls.
    assert stats.bootstrap.vendor.total_input_tokens == 10
    assert stats.driven.vendor.total_input_tokens == 20 + 30 + 40
    assert stats.final.vendor.total_input_tokens == 50
    assert (
        stats.bootstrap.vendor.total_input_tokens
        + stats.driven.vendor.total_input_tokens
        + stats.final.vendor.total_input_tokens
        == stats.vendor.total_input_tokens
    )


def test_three_way_split_codex_like_pattern() -> None:
    """Codex's typical shape: bootstrap event #0 already carries tool_calls.
    Expected: bootstrap empty, driven = [#0..#n-2], final = last."""

    call_n = 0

    async def handler(request: web.Request) -> web.Response:
        nonlocal call_n
        call_n += 1
        if call_n < 4:
            output = [
                {"type": "function_call", "name": "sh", "arguments": '{"cmd":"ls"}'},
            ]
        else:
            output = [
                {"type": "message", "content": [{"type": "output_text", "text": "final"}]},
            ]
        return web.json_response(
            {
                "output": output,
                "usage": {"input_tokens": 10 * call_n, "output_tokens": call_n},
            }
        )

    app = web.Application()
    app.router.add_post("/responses", handler)

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="openai", upstream_base=srv.base_url)
        for _ in range(4):
            _http_post(
                f"{session.base_url}/responses",
                b'{"input":[{"role":"user","content":[{"type":"input_text","text":"hi"}]}]}',
                {"Content-Type": "application/json"},
            )
        stats = gw.get_stats(session.id)

    assert stats.loop_start_index == 0
    assert stats.bootstrap.event_count == 0
    assert stats.driven.event_count == 3
    assert stats.final.event_count == 1


def test_no_tool_calls_flow_is_bootstrap_then_final() -> None:
    """If the model never issues a tool_call, driven is empty and the
    session is split bootstrap → final."""

    async def handler(request: web.Request) -> web.Response:
        return web.json_response(
            {
                "content": [{"type": "text", "text": "answer"}],
                "usage": {"input_tokens": 5, "output_tokens": 1},
            }
        )

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        for _ in range(2):  # pre-flight + answer, both text-only
            _http_post(
                f"{session.base_url}/v1/messages",
                b'{"messages":[{"role":"user","content":"hi"}]}',
                {"Content-Type": "application/json"},
            )
        stats = gw.get_stats(session.id)

    assert stats.loop_start_index == 2  # never found one
    assert stats.bootstrap.event_count == 1
    assert stats.driven.event_count == 0
    assert stats.final.event_count == 1


def test_single_event_session_goes_entirely_to_final() -> None:
    """One-event session: the single event is the final output; bootstrap
    and driven are empty by construction."""

    async def handler(request: web.Request) -> web.Response:
        return web.json_response(
            {
                "content": [{"type": "text", "text": "ok"}],
                "usage": {"input_tokens": 3, "output_tokens": 1},
            }
        )

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        _http_post(
            f"{session.base_url}/v1/messages",
            b'{"messages":[{"role":"user","content":"hi"}]}',
            {"Content-Type": "application/json"},
        )
        stats = gw.get_stats(session.id)

    assert stats.bootstrap.event_count == 0
    assert stats.driven.event_count == 0
    assert stats.final.event_count == 1
    assert stats.final.vendor.total_input_tokens == 3


def test_single_prompt_session_yields_one_turn_matching_session_splits() -> None:
    """Invariant: for a single-prompt session (what benchmark runs
    produce), ``stats.turns`` has length 1 and the one turn's
    bootstrap/driven/final match the session-wide split."""

    call_n = 0

    async def handler(request: web.Request) -> web.Response:
        nonlocal call_n
        call_n += 1
        if call_n == 1:
            content = [{"type": "text", "text": "title"}]
        elif call_n in (2, 3):
            content = [{"type": "tool_use", "id": f"T{call_n}", "name": "Read", "input": {}}]
        else:
            content = [{"type": "text", "text": "final answer"}]
        return web.json_response(
            {"content": content, "usage": {"input_tokens": 10 * call_n, "output_tokens": call_n}}
        )

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    # All events share the same single user prompt — no multi-turn.
    body = json.dumps({"messages": [{"role": "user", "content": "explain"}]}).encode()

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        for _ in range(4):
            _http_post(
                f"{session.base_url}/v1/messages",
                body,
                {"Content-Type": "application/json"},
            )
        stats = gw.get_stats(session.id)

    assert len(stats.turns) == 1
    t0 = stats.turns[0]
    assert t0.turn_index == 0
    assert len(t0.events) == len(stats.events)
    # Per-turn slices must match session-wide slices byte-for-byte.
    for attr in ("event_count", "raw_request_bytes", "raw_response_bytes"):
        for slice_name in ("bootstrap", "driven", "final"):
            assert getattr(getattr(t0, slice_name), attr) == getattr(
                getattr(stats, slice_name), attr
            )
    assert t0.bootstrap.vendor.total_input_tokens == stats.bootstrap.vendor.total_input_tokens


def test_multi_turn_splits_at_fresh_prompt_after_no_tool_call() -> None:
    """Two-turn session: user prompt 1 → tool loop → answer 1 → user
    prompt 2 → tool loop → answer 2. stats.turns reports 2 turns."""

    call_n = 0

    async def handler(request: web.Request) -> web.Response:
        nonlocal call_n
        call_n += 1
        # Turn 1: events 1 (tool_use) → 2 (answer)
        # Turn 2: events 3 (tool_use) → 4 (answer)
        if call_n in (1, 3):
            content = [{"type": "tool_use", "id": f"T{call_n}", "name": "Read", "input": {}}]
        else:
            content = [{"type": "text", "text": f"answer{call_n}"}]
        return web.json_response(
            {"content": content, "usage": {"input_tokens": 10 * call_n, "output_tokens": call_n}}
        )

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    # Turn 1 bootstrap request (fresh prompt #1).
    body_t1_bs = json.dumps({"messages": [{"role": "user", "content": "prompt one"}]}).encode()
    # Turn 1 driven (same prompt + tool_use history + tool_result).
    body_t1_dr = json.dumps(
        {
            "messages": [
                {"role": "user", "content": "prompt one"},
                {
                    "role": "assistant",
                    "content": [{"type": "tool_use", "id": "T1", "name": "Read", "input": {}}],
                },
                {
                    "role": "user",
                    "content": [{"type": "tool_result", "tool_use_id": "T1", "content": "R1"}],
                },
            ]
        }
    ).encode()
    # Turn 2 bootstrap (prompt one + assistant answer + fresh prompt #2).
    body_t2_bs = json.dumps(
        {
            "messages": [
                {"role": "user", "content": "prompt one"},
                {"role": "assistant", "content": [{"type": "text", "text": "answer2"}]},
                {"role": "user", "content": "prompt two"},
            ]
        }
    ).encode()
    body_t2_dr = json.dumps(
        {
            "messages": [
                {"role": "user", "content": "prompt one"},
                {"role": "assistant", "content": [{"type": "text", "text": "answer2"}]},
                {"role": "user", "content": "prompt two"},
                {
                    "role": "assistant",
                    "content": [{"type": "tool_use", "id": "T3", "name": "Read", "input": {}}],
                },
                {
                    "role": "user",
                    "content": [{"type": "tool_result", "tool_use_id": "T3", "content": "R2"}],
                },
            ]
        }
    ).encode()

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        for body in (body_t1_bs, body_t1_dr, body_t2_bs, body_t2_dr):
            _http_post(
                f"{session.base_url}/v1/messages",
                body,
                {"Content-Type": "application/json"},
            )
        stats = gw.get_stats(session.id)

    # Turns: [0..1] (turn 1: bootstrap + answer), [2..3] (turn 2).
    assert stats.turn_start_indices == [0, 2]
    assert len(stats.turns) == 2
    assert [len(t.events) for t in stats.turns] == [2, 2]

    # Each turn has its own [bootstrap, driven, final] split.
    t0, t1 = stats.turns
    # Turn 1: event 0 = tool_use response (loop start), event 1 = answer.
    assert t0.loop_start_index == 0
    assert t0.bootstrap.event_count == 0
    assert t0.driven.event_count == 1
    assert t0.final.event_count == 1
    # Turn 2: same pattern.
    assert t1.loop_start_index == 0
    assert t1.driven.event_count == 1
    assert t1.final.event_count == 1


def test_claude_code_style_system_reminders_dont_split_turns() -> None:
    """Claude-Code injects ``<system-reminder>`` blocks in the user role
    on every turn. These must NOT be treated as fresh user prompts."""

    async def handler(request: web.Request) -> web.Response:
        return web.json_response(
            {
                "content": [{"type": "tool_use", "id": "T1", "name": "Read", "input": {}}],
                "usage": {"input_tokens": 5, "output_tokens": 1},
            }
        )

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    # Each turn adds a NEW sysreminder but the actual user prompt is stable.
    bodies = [
        json.dumps(
            {
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {"type": "text", "text": "<system-reminder>alpha</system-reminder>"},
                            {"type": "text", "text": "my real prompt"},
                        ],
                    }
                ]
            }
        ).encode(),
        json.dumps(
            {
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "<system-reminder>beta different</system-reminder>",
                            },
                            {"type": "text", "text": "my real prompt"},
                        ],
                    },
                    {
                        "role": "assistant",
                        "content": [{"type": "tool_use", "id": "T1", "name": "Read", "input": {}}],
                    },
                    {
                        "role": "user",
                        "content": [{"type": "tool_result", "tool_use_id": "T1", "content": "R"}],
                    },
                ]
            }
        ).encode(),
    ]

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        for body in bodies:
            _http_post(
                f"{session.base_url}/v1/messages",
                body,
                {"Content-Type": "application/json"},
            )
        stats = gw.get_stats(session.id)

    # Both events share one real user prompt — single turn only.
    assert stats.turn_start_indices == [0]
    assert len(stats.turns) == 1


def test_subagent_style_invocation_does_not_split_turns() -> None:
    """When the parent agent issues a tool_use (e.g. spawning a
    subagent) and the next request carries a fresh user-text (the
    subagent's prompt), the turn must NOT split because the previous
    event was mid-loop (had a tool_call in response)."""

    call_n = 0

    async def handler(request: web.Request) -> web.Response:
        nonlocal call_n
        call_n += 1
        if call_n == 1:
            # Parent: tool_use to "delegate".
            content = [{"type": "tool_use", "id": "T1", "name": "Agent", "input": {}}]
        else:
            content = [{"type": "text", "text": "final"}]
        return web.json_response(
            {"content": content, "usage": {"input_tokens": 5, "output_tokens": 1}}
        )

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    # Event 0: bootstrap with prompt #1, model emits tool_use.
    body0 = json.dumps({"messages": [{"role": "user", "content": "parent prompt"}]}).encode()
    # Event 1: "subagent" request with a BRAND-NEW user text — but the
    # previous event had a tool_call, so this is NOT a turn boundary.
    body1 = json.dumps(
        {"messages": [{"role": "user", "content": "subagent prompt for reading files"}]}
    ).encode()

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        for body in (body0, body1):
            _http_post(
                f"{session.base_url}/v1/messages",
                body,
                {"Content-Type": "application/json"},
            )
        stats = gw.get_stats(session.id)

    assert stats.turn_start_indices == [0]
    assert len(stats.turns) == 1


def test_timing_fields_and_generation_tok_per_s() -> None:
    """Event timing splits into ttft + generation. The mock upstream
    holds the response open after writing the full body so we can
    observe non-zero TTFT and a non-zero gap between events."""

    async def handler(request: web.Request) -> web.StreamResponse:
        resp = web.StreamResponse(status=200, headers={"Content-Type": "text/event-stream"})
        resp.enable_chunked_encoding()
        await resp.prepare(request)
        # Artificial prefill delay before first byte.
        await asyncio.sleep(0.15)
        events = [
            {
                "type": "message_start",
                "message": {"id": "m1", "usage": {"input_tokens": 5, "output_tokens": 1}},
            },
            {
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""},
            },
            {
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "hi"},
            },
            {"type": "content_block_stop", "index": 0},
            {
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 10},
            },
        ]
        for ev in events:
            await resp.write(
                f"event: {ev['type']}\n".encode() + b"data: " + json.dumps(ev).encode() + b"\n\n"
            )
            # Stagger chunks so generation_seconds > 0.
            await asyncio.sleep(0.02)
        await resp.write_eof()
        return resp

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        # First event.
        _http_post(
            f"{session.base_url}/v1/messages",
            b'{"messages":[{"role":"user","content":"a"}]}',
            {"Content-Type": "application/json"},
        )
        # Simulated harness gap before second event.
        import time as _t

        _t.sleep(0.2)
        _http_post(
            f"{session.base_url}/v1/messages",
            b'{"messages":[{"role":"user","content":"a"}]}',
            {"Content-Type": "application/json"},
        )
        stats = gw.get_stats(session.id)

    assert len(stats.events) == 2
    e0, e1 = stats.events

    # Event 0: first response byte captured, TTFT ≥ ~0.15s, gap = 0 (first event).
    assert e0.first_response_byte_at is not None
    assert e0.ttft_seconds is not None and e0.ttft_seconds >= 0.10
    assert e0.generation_seconds is not None and e0.generation_seconds > 0
    assert e0.harness_gap_seconds == 0.0

    # Event 1: harness gap captures the sleep.
    assert e1.harness_gap_seconds >= 0.15

    # Slice aggregates include timings.
    assert stats.bootstrap.event_count + stats.driven.event_count + stats.final.event_count == 2
    full_wall = (
        stats.bootstrap.wall_clock_seconds
        + stats.driven.wall_clock_seconds
        + stats.final.wall_clock_seconds
    )
    assert full_wall >= e0.wall_clock_seconds + e1.wall_clock_seconds - 1e-6

    # tok/s is computable from generation_seconds only (not total wall clock).
    final_slice = stats.final
    if final_slice.generation_tok_per_s is not None:
        assert final_slice.generation_tok_per_s > 0


def test_session_novel_content_dedupes_replays() -> None:
    """Wire-level content_to_model sums replayed history; novel_content_to_model
    counts each tool_result and tool_use exactly once per session."""

    async def handler(request: web.Request) -> web.Response:
        return web.json_response(
            {
                "content": [{"type": "text", "text": "ok"}],
                "usage": {"input_tokens": 1, "output_tokens": 1},
            }
        )

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    req_bodies = [
        # Turn 1: initial bootstrap
        {
            "system": "SYS",
            "tools": [{"name": "Read", "description": "d"}],
            "messages": [{"role": "user", "content": "go"}],
        },
        # Turn 2: replays turn 1 and adds a tool_use + tool_result
        {
            "system": "SYS",
            "tools": [{"name": "Read", "description": "d"}],
            "messages": [
                {"role": "user", "content": "go"},
                {
                    "role": "assistant",
                    "content": [{"type": "tool_use", "id": "T1", "name": "Read", "input": {}}],
                },
                {
                    "role": "user",
                    "content": [
                        {"type": "tool_result", "tool_use_id": "T1", "content": "RESULT_X"}
                    ],
                },
            ],
        },
        # Turn 3: replays turns 1 & 2, plus new T2
        {
            "system": "SYS",
            "tools": [{"name": "Read", "description": "d"}],
            "messages": [
                {"role": "user", "content": "go"},
                {
                    "role": "assistant",
                    "content": [
                        {"type": "tool_use", "id": "T1", "name": "Read", "input": {}},
                        {"type": "tool_use", "id": "T2", "name": "Read", "input": {}},
                    ],
                },
                {
                    "role": "user",
                    "content": [
                        {"type": "tool_result", "tool_use_id": "T1", "content": "RESULT_X"},
                        {"type": "tool_result", "tool_use_id": "T2", "content": "RESULT_Y_LONGER"},
                    ],
                },
            ],
        },
    ]

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        for body in req_bodies:
            _http_post(
                f"{session.base_url}/v1/messages",
                json.dumps(body).encode(),
                {"Content-Type": "application/json"},
            )
        stats = gw.get_stats(session.id)

    # Wire content counts each replay.
    assert stats.content_to_model.tool_result_count == 0 + 1 + 2
    assert stats.content_to_model.tool_call_count == 0 + 1 + 2

    # Novel deduplicates: 2 unique tool_uses (T1, T2), 2 unique tool_results.
    assert stats.novel_content_to_model.tool_call_count == 2
    assert stats.novel_content_to_model.tool_result_count == 2
    assert stats.novel_content_to_model.tool_result_bytes == len(b"RESULT_X") + len(
        b"RESULT_Y_LONGER"
    )
    # System prompt and tool schema appear once each.
    assert stats.novel_content_to_model.system_prompt_bytes == len(b"SYS")
    assert (
        stats.novel_content_to_model.tool_schema_bytes
        == stats.content_to_model.tool_schema_bytes // 3
    )

    # Novel is always ≤ wire.
    for f in (
        "user_text_bytes",
        "tool_result_bytes",
        "assistant_text_bytes",
        "tool_call_bytes",
        "reasoning_bytes",
        "system_prompt_bytes",
        "tool_schema_bytes",
    ):
        assert getattr(stats.novel_content_to_model, f) <= getattr(stats.content_to_model, f)


def test_novel_splits_cleanly_across_bootstrap_driven_final() -> None:
    """Sum of per-phase novel == session novel (dedup is order-preserving)."""

    async def handler(request: web.Request) -> web.Response:
        return web.json_response(
            {
                "content": [{"type": "text", "text": "ok"}],
                "usage": {"input_tokens": 1, "output_tokens": 1},
            }
        )

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)

        # Seed a few turns with tool_uses introduced progressively.
        turns = [
            # event #0: bootstrap-style, no tool_use/result yet, response pure text
            {
                "system": "SYS",
                "tools": [{"name": "Read", "description": "d"}],
                "messages": [{"role": "user", "content": "start"}],
            },
            # event #1: add first tool_use+result
            {
                "system": "SYS",
                "messages": [
                    {"role": "user", "content": "start"},
                    {
                        "role": "assistant",
                        "content": [{"type": "tool_use", "id": "T1", "name": "Read", "input": {}}],
                    },
                    {
                        "role": "user",
                        "content": [{"type": "tool_result", "tool_use_id": "T1", "content": "R1"}],
                    },
                ],
            },
            # event #2: final turn, add T2
            {
                "system": "SYS",
                "messages": [
                    {"role": "user", "content": "start"},
                    {
                        "role": "assistant",
                        "content": [
                            {"type": "tool_use", "id": "T1", "name": "Read", "input": {}},
                            {"type": "tool_use", "id": "T2", "name": "Read", "input": {}},
                        ],
                    },
                    {
                        "role": "user",
                        "content": [
                            {"type": "tool_result", "tool_use_id": "T1", "content": "R1"},
                            {"type": "tool_result", "tool_use_id": "T2", "content": "R2"},
                        ],
                    },
                ],
            },
        ]
        for body in turns:
            _http_post(
                f"{session.base_url}/v1/messages",
                json.dumps(body).encode(),
                {"Content-Type": "application/json"},
            )
        stats = gw.get_stats(session.id)

    assert (
        stats.bootstrap.novel_content_to_model.tool_result_count
        + stats.driven.novel_content_to_model.tool_result_count
        + stats.final.novel_content_to_model.tool_result_count
        == stats.novel_content_to_model.tool_result_count
    )
    assert (
        stats.bootstrap.novel_content_to_model.tool_result_bytes
        + stats.driven.novel_content_to_model.tool_result_bytes
        + stats.final.novel_content_to_model.tool_result_bytes
        == stats.novel_content_to_model.tool_result_bytes
    )


def test_content_breakdown_tracks_request_and_response_bytes() -> None:
    """End-to-end: request body is parsed for content buckets, and the
    SSE stream analyzer accumulates response buckets, so callers can
    compare text-only traffic across vendors."""

    async def handler(request: web.Request) -> web.StreamResponse:
        resp = web.StreamResponse(status=200, headers={"Content-Type": "text/event-stream"})
        resp.enable_chunked_encoding()
        await resp.prepare(request)
        events = [
            {
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""},
            },
            {
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "answer"},
            },
            {"type": "content_block_stop", "index": 0},
            {
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 3},
            },
        ]
        for ev in events:
            await resp.write(
                f"event: {ev['type']}\n".encode() + b"data: " + json.dumps(ev).encode() + b"\n\n"
            )
        await resp.write_eof()
        return resp

    app = web.Application()
    app.router.add_post("/v1/messages", handler)

    req_body = json.dumps(
        {
            "model": "claude-sonnet-4-6",
            "max_tokens": 1024,
            "system": "you are helpful",
            "messages": [
                {"role": "user", "content": "hello model"},
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "tool_result",
                            "tool_use_id": "t1",
                            "content": [{"type": "text", "text": "file bytes here"}],
                        }
                    ],
                },
            ],
            "tools": [{"name": "read", "description": "reads files"}],
        }
    ).encode()

    with _upstream(app) as srv, _gateway() as gw:
        session = gw.create_session(upstream="anthropic", upstream_base=srv.base_url)
        _http_post(
            f"{session.base_url}/v1/messages",
            req_body,
            {"Content-Type": "application/json"},
        )
        stats = gw.get_stats(session.id)

    assert stats.content_to_model.system_prompt_bytes == len(b"you are helpful")
    assert stats.content_to_model.user_text_bytes == len(b"hello model")
    assert stats.content_to_model.tool_result_bytes == len(b"file bytes here")
    assert stats.content_to_model.tool_schema_bytes > 0
    assert stats.content_from_model.assistant_text_bytes == len(b"answer")
    # raw bytes are still available for transport sanity.
    assert stats.raw_request_bytes == len(req_body)
    assert stats.raw_response_bytes > 0
