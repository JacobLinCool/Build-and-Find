"""Replay recorded upstream sessions through the Gateway.

``Gateway(dump_dir=...)`` writes one ``NNNN.req`` / ``NNNN.resp`` /
``NNNN.meta`` triple per event. :class:`ReplayUpstream` reads that
directory and serves the recorded responses in arrival order, and
:func:`replay_through_gateway` drives the gateway with the recorded
request bodies.

Together they give us a deterministic, zero-cost regression harness:
capture a real session once, replay it in CI forever.
"""

from __future__ import annotations

import asyncio
import json
import re
import threading
import urllib.request
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Iterator, Optional

from aiohttp import web

from bnf.proxy import Gateway, SessionHandle, SessionStats


_META_RE = re.compile(r"^(?P<method>[A-Z]+)\s+(?P<path>\S+)\s*$")


@dataclass
class RecordedEvent:
    index: int
    method: str
    path: str
    status: int
    content_type: str
    request_body: bytes
    response_body: bytes


def load_fixture(dump_dir: Path) -> list[RecordedEvent]:
    """Load all ``NNNN.{req,resp,meta}`` triples from a dump directory,
    ordered by index."""

    events: list[RecordedEvent] = []
    meta_files = sorted(dump_dir.glob("*.meta"))
    for meta_path in meta_files:
        idx = int(meta_path.stem)
        lines = meta_path.read_text(encoding="utf-8").splitlines()
        first = _META_RE.match(lines[0]) if lines else None
        if first is None:
            raise ValueError(f"cannot parse method/path from {meta_path}")
        status = 200
        content_type = ""
        for line in lines[1:]:
            if "=" not in line:
                continue
            key, value = line.split("=", 1)
            key = key.strip()
            value = value.strip()
            if key == "status":
                status = int(value)
            elif key == "content-type":
                content_type = value
        req_path = dump_dir / f"{idx:04d}.req"
        resp_path = dump_dir / f"{idx:04d}.resp"
        events.append(
            RecordedEvent(
                index=idx,
                method=first["method"],
                path=first["path"],
                status=status,
                content_type=content_type,
                request_body=req_path.read_bytes(),
                response_body=resp_path.read_bytes(),
            )
        )
    return events


class ReplayUpstream:
    """``aiohttp`` server that plays back a sequence of recorded
    responses in arrival order.

    The server ignores the incoming request path — it trusts the
    gateway to hand requests over in the same order they were
    originally issued. That keeps the harness tiny; add path matching
    if you need random-access replay later.
    """

    def __init__(self, events: list[RecordedEvent]) -> None:
        self._events = events
        self._idx = 0
        self._loop: Optional[asyncio.AbstractEventLoop] = None
        self._runner: Optional[web.AppRunner] = None
        self._site: Optional[web.TCPSite] = None
        self._thread: Optional[threading.Thread] = None
        self._ready = threading.Event()
        self.port: int = 0
        self._idx_lock = threading.Lock()

    @property
    def base_url(self) -> str:
        return f"http://127.0.0.1:{self.port}"

    def start(self) -> None:
        import socket

        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.bind(("127.0.0.1", 0))
        self.port = s.getsockname()[1]
        s.close()
        self._thread = threading.Thread(target=self._run, daemon=True, name="replay-upstream")
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
        app = web.Application()
        app.router.add_route("*", "/{tail:.*}", self._handler)
        self._runner = web.AppRunner(app, access_log=None)
        await self._runner.setup()
        self._site = web.TCPSite(self._runner, "127.0.0.1", self.port)
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

    async def _handler(self, request: web.Request) -> web.StreamResponse:
        # Drain the request body (we don't compare it — the gateway
        # already sees it) so the client doesn't stall.
        await request.read()
        with self._idx_lock:
            if self._idx >= len(self._events):
                return web.json_response({"error": "replay exhausted"}, status=500)
            evt = self._events[self._idx]
            self._idx += 1
        headers = {}
        if evt.content_type:
            headers["Content-Type"] = evt.content_type

        if "text/event-stream" in evt.content_type:
            resp = web.StreamResponse(status=evt.status, headers=headers)
            resp.enable_chunked_encoding()
            await resp.prepare(request)
            # Chunk the recorded bytes to simulate a streamed response.
            body = evt.response_body
            chunk = 4096
            for i in range(0, len(body), chunk):
                await resp.write(body[i : i + chunk])
            await resp.write_eof()
            return resp
        return web.Response(status=evt.status, body=evt.response_body, headers=headers)


@contextmanager
def replay_context(events: list[RecordedEvent]) -> Iterator["_Context"]:
    """Spin up a ReplayUpstream + Gateway + session pointed at it.

    Yields a small bundle so tests can drive the recorded sequence
    without boilerplate.
    """

    upstream = ReplayUpstream(events)
    upstream.start()
    gw: Optional[Gateway] = None
    try:
        gw = Gateway.start()
        # Heuristic: Anthropic fixtures hit /v1/messages; OpenAI
        # fixtures hit /responses (no /v1 prefix). Fall back to
        # ``anthropic`` by default.
        first_path = events[0].path if events else "/v1/messages"
        upstream_name = "openai" if first_path.startswith("/responses") else "anthropic"
        session = gw.create_session(upstream=upstream_name, upstream_base=upstream.base_url)
        yield _Context(gateway=gw, session=session, events=events)
    finally:
        if gw is not None:
            gw.stop()
        upstream.stop()


@dataclass
class _Context:
    gateway: Gateway
    session: SessionHandle
    events: list[RecordedEvent]

    def drive(self) -> SessionStats:
        """Replay every recorded request through the gateway and return
        the final :class:`SessionStats`."""

        for evt in self.events:
            url = f"{self.session.base_url}{evt.path}"
            req = urllib.request.Request(url, data=evt.request_body or None, method=evt.method)
            # Set a minimal content-type on the request; the gateway
            # forwards this to the replay upstream, which doesn't
            # actually read it, so correctness of this header doesn't
            # matter for replay tests.
            req.add_header("Content-Type", "application/json")
            with urllib.request.urlopen(req, timeout=30) as resp:
                resp.read()
        return self.gateway.get_stats(self.session.id)


def make_synthetic_anthropic_fixture(
    turns: Iterable[dict],
) -> list[RecordedEvent]:
    """Build an in-memory anthropic fixture from a minimal description.

    Each ``turn`` is a dict with ``request`` (a full Anthropic
    ``/v1/messages`` request body as a python dict) and ``response`` (a
    python dict matching the JSON shape of a non-streaming Anthropic
    response). Usage metrics default to a small stable value.
    """

    events: list[RecordedEvent] = []
    for i, turn in enumerate(turns):
        req = json.dumps(turn["request"]).encode()
        resp_obj = dict(turn.get("response", {}))
        resp_obj.setdefault("usage", {"input_tokens": 10, "output_tokens": 1})
        resp = json.dumps(resp_obj).encode()
        events.append(
            RecordedEvent(
                index=i,
                method="POST",
                path="/v1/messages",
                status=200,
                content_type="application/json",
                request_body=req,
                response_body=resp,
            )
        )
    return events
