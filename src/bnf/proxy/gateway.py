"""Pass-through HTTP proxy with per-session usage tracking.

The :class:`Gateway` runs an ``aiohttp`` server on a background thread with
its own asyncio loop, so callers can drive it from ordinary synchronous
code (including subprocess orchestration).

Each tracked experiment gets a :class:`SessionHandle` whose ``base_url``
looks like ``http://127.0.0.1:<port>/s/<session_id>``. Setting that value
as ``ANTHROPIC_BASE_URL`` (or ``OPENAI_BASE_URL``) routes the agent's
traffic through the proxy, which transparently forwards it to
``api.anthropic.com`` / ``api.openai.com`` while counting bytes and
parsing vendor usage.
"""

from __future__ import annotations

import asyncio
import dataclasses
import secrets
import socket
import threading
import time
from dataclasses import dataclass, field
from typing import Optional

import aiohttp
from aiohttp import web

from bnf.proxy.parser import (
    ContentBreakdown,
    NovelTracker,
    StreamContentAnalyzer,
    StreamUsageParser,
    UsageSnapshot,
    analyze_request_body,
    analyze_request_body_novel,
    analyze_response_body,
    count_fresh_user_prompts,
    parse_json_usage,
)

UPSTREAMS: dict[str, str] = {
    "anthropic": "https://api.anthropic.com",
    "openai": "https://api.openai.com",
    # Codex in ChatGPT-login mode talks to the ChatGPT backend instead of
    # ``api.openai.com``. The Responses-compatible path underneath is
    # ``/responses`` (no ``/v1`` prefix).
    "chatgpt": "https://chatgpt.com/backend-api/codex",
    # Xiaomi MiMo Token-Plan tier exposes an Anthropic-compatible endpoint
    # at ``/anthropic/v1/messages``. Used by the ``claude_code_mimo_*``
    # adapters with ``ANTHROPIC_AUTH_TOKEN`` (Bearer key) auth.
    "xiaomimimo": "https://token-plan-sgp.xiaomimimo.com/anthropic",
}

# Which parser to use for each upstream. ChatGPT backend speaks the same
# OpenAI Responses wire format. xiaomimimo is wire-compatible with the
# Anthropic Messages API.
_PARSER_FAMILY: dict[str, str] = {
    "anthropic": "anthropic",
    "openai": "openai",
    "chatgpt": "openai",
    "xiaomimimo": "anthropic",
}

# Headers that must not be blindly forwarded. Content-Length and
# Content-Encoding are excluded because aiohttp auto-decompresses the
# upstream body and will re-frame the hop to the client.
_SKIP_REQUEST_HEADERS = frozenset(
    {
        "host",
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailers",
        "transfer-encoding",
        "upgrade",
        "content-length",
    }
)
_SKIP_RESPONSE_HEADERS = frozenset(
    {
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailers",
        "transfer-encoding",
        "upgrade",
        "content-length",
        "content-encoding",
    }
)


@dataclass
class VendorCounter:
    """Running sum of vendor-reported tokens across one session.

    Field semantics match :class:`~bnf.proxy.parser.UsageSnapshot` — the
    input bucket is normalized so Anthropic and OpenAI numbers mean the
    same thing (total input = cached + uncached), and output is split
    into non_reasoning / reasoning.
    """

    total_input_tokens: int = 0
    cached_input_tokens: int = 0
    uncached_input_tokens: int = 0
    cache_creation_input_tokens: int = 0
    non_reasoning_output_tokens: int = 0
    reasoning_output_tokens: int = 0
    total_tokens: int = 0

    def add(self, snap: UsageSnapshot) -> None:
        self.total_input_tokens += snap.total_input_tokens
        self.cached_input_tokens += snap.cached_input_tokens
        self.uncached_input_tokens += snap.uncached_input_tokens
        self.cache_creation_input_tokens += snap.cache_creation_input_tokens
        self.non_reasoning_output_tokens += snap.non_reasoning_output_tokens
        self.reasoning_output_tokens += snap.reasoning_output_tokens
        self.total_tokens += snap.total_tokens


@dataclass
class ProxyEvent:
    """One proxied HTTP request/response pair.

    ``raw_request_bytes`` / ``raw_response_bytes`` record the on-the-wire
    body sizes (everything, including JSON framing and model parameters)
    and are useful only for transport-level sanity checks.

    For cross-vendor comparability, use ``content_to_model`` and
    ``content_from_model``: text-leaf-only bytes bucketed by role.
    """

    index: int
    method: str
    path: str
    status: int
    raw_request_bytes: int
    raw_response_bytes: int
    started_at: float
    completed_at: float
    streaming: bool
    content_type: str = ""
    # Wall time of the first response byte from the upstream. Subtracting
    # ``started_at`` gives TTFT (upload + upstream prefill); subtracting
    # from ``completed_at`` gives pure generation time.
    first_response_byte_at: Optional[float] = None
    # ``started_at`` of this event minus ``completed_at`` of the previous
    # event — the time the harness spent running tools / preparing the
    # next request between model calls. Zero for the first event.
    harness_gap_seconds: float = 0.0
    # Number of user-authored prompts newly appearing in this event's
    # request (after filtering harness injections like
    # ``<system-reminder>``). Used by :attr:`SessionStats.turns` to
    # detect multi-turn boundaries.
    fresh_user_prompts: int = 0
    content_to_model: ContentBreakdown = field(default_factory=ContentBreakdown)
    content_from_model: ContentBreakdown = field(default_factory=ContentBreakdown)
    # Subset of ``content_to_model`` holding only blocks that are
    # first-appearing in the session. Every later event replaying the
    # same block contributes zero here — summing this field across a
    # slice yields the novel retrieval effort for that slice.
    novel_content_to_model: ContentBreakdown = field(default_factory=ContentBreakdown)
    usage: Optional[dict[str, int]] = None
    error: Optional[str] = None

    @property
    def wall_clock_seconds(self) -> float:
        return max(0.0, self.completed_at - self.started_at)

    @property
    def ttft_seconds(self) -> Optional[float]:
        if self.first_response_byte_at is None:
            return None
        return max(0.0, self.first_response_byte_at - self.started_at)

    @property
    def generation_seconds(self) -> Optional[float]:
        if self.first_response_byte_at is None:
            return None
        return max(0.0, self.completed_at - self.first_response_byte_at)


@dataclass
class SessionHandle:
    id: str
    base_url: str
    upstream: str


@dataclass
class EventSlice:
    """Aggregated vendor + content over a subset of a session's events.

    ``content_to_model`` is wire-level (includes history replay);
    ``novel_content_to_model`` is the dedupe-by-ID slice of the same —
    each tool_result / tool_use / text hash is counted at most once
    across the entire session, at the event where it first appeared.
    """

    event_count: int = 0
    raw_request_bytes: int = 0
    raw_response_bytes: int = 0
    #: Sum of per-event wall clock: request send → last response byte.
    wall_clock_seconds: float = 0.0
    #: Sum of per-event TTFT (upload + upstream prefill). ``None`` is
    #: treated as 0 so callers can still divide cleanly.
    ttft_seconds: float = 0.0
    #: Sum of per-event pure generation time (first response byte →
    #: completed). Divide output tokens by this for tok/s.
    generation_seconds: float = 0.0
    #: Sum of per-event harness gaps (tool execution + prep between
    #: model calls).
    harness_gap_seconds: float = 0.0
    vendor: VendorCounter = field(default_factory=VendorCounter)
    content_to_model: ContentBreakdown = field(default_factory=ContentBreakdown)
    novel_content_to_model: ContentBreakdown = field(default_factory=ContentBreakdown)
    content_from_model: ContentBreakdown = field(default_factory=ContentBreakdown)

    @property
    def generation_tok_per_s(self) -> Optional[float]:
        """Output tokens (non-reasoning + reasoning) per second of pure
        generation. ``None`` if no generation time was captured."""

        if self.generation_seconds <= 0:
            return None
        out_tokens = self.vendor.non_reasoning_output_tokens + self.vendor.reasoning_output_tokens
        if out_tokens == 0:
            return None
        return out_tokens / self.generation_seconds


@dataclass
class SessionStats:
    id: str
    upstream: str
    base_url: str
    requests: int
    raw_request_bytes: int
    raw_response_bytes: int
    first_request_at: Optional[float]
    last_request_at: Optional[float]
    wall_clock_seconds: float
    vendor: VendorCounter
    content_to_model: ContentBreakdown
    novel_content_to_model: ContentBreakdown
    content_from_model: ContentBreakdown
    events: list[ProxyEvent] = field(default_factory=list)

    # ------------------------------------------------------------------
    # Bootstrap / driven / final split — session-wide view
    # ------------------------------------------------------------------

    @property
    def loop_start_index(self) -> int:
        """Index of the first event whose RESPONSE carries a tool_call.
        Same rule applied across the whole session regardless of
        multi-turn boundaries. See :attr:`turns` for per-turn views.
        """

        return _loop_start_of(self.events)

    @property
    def bootstrap(self) -> EventSlice:
        return _bootstrap_of(self.events)

    @property
    def driven(self) -> EventSlice:
        return _driven_of(self.events)

    @property
    def final(self) -> EventSlice:
        return _final_of(self.events)

    # ------------------------------------------------------------------
    # Multi-turn view
    # ------------------------------------------------------------------

    @property
    def turn_start_indices(self) -> list[int]:
        """Event indices that start a new user turn. Always includes 0
        when the session has events. See :func:`_detect_turn_starts` for
        the heuristic; in short: a fresh user prompt after an event
        whose response had no tool_call."""

        return _detect_turn_starts(self.events)

    @property
    def turns(self) -> list["TurnSlice"]:
        """Per-turn slices. Single-prompt sessions yield ``len == 1``
        with ``turns[0]`` identical (by events) to the session-wide
        view; bootstrap / driven / final computed on ``turns[0].events``
        matches ``self.bootstrap / .driven / .final``.
        """

        starts = self.turn_start_indices
        if not starts:
            return []
        out: list[TurnSlice] = []
        for turn_idx, start in enumerate(starts):
            end = starts[turn_idx + 1] if turn_idx + 1 < len(starts) else len(self.events)
            out.append(TurnSlice(turn_index=turn_idx, events=list(self.events[start:end])))
        return out


@dataclass
class TurnSlice:
    """Events belonging to one user turn in a multi-turn session.

    Exposes the same ``bootstrap / driven / final`` split as
    :class:`SessionStats`, computed over the turn's own events. A
    single-prompt session yields exactly one TurnSlice that covers
    every event; its splits match the session-wide ones by
    construction.
    """

    turn_index: int
    events: list[ProxyEvent]

    @property
    def loop_start_index(self) -> int:
        return _loop_start_of(self.events)

    @property
    def bootstrap(self) -> EventSlice:
        return _bootstrap_of(self.events)

    @property
    def driven(self) -> EventSlice:
        return _driven_of(self.events)

    @property
    def final(self) -> EventSlice:
        return _final_of(self.events)


# ----------------------------------------------------------------------
# Shared split helpers used by both SessionStats and TurnSlice
# ----------------------------------------------------------------------


def _loop_start_of(events: list[ProxyEvent]) -> int:
    # Use the explicit count, not bytes — a tool_call with empty args
    # has zero bytes but still counts as a decision.
    for i, evt in enumerate(events):
        if evt.content_from_model.tool_call_count > 0:
            return i
    return len(events)


def _bootstrap_of(events: list[ProxyEvent]) -> EventSlice:
    if not events:
        return _summarize_events([])
    end = min(_loop_start_of(events), len(events) - 1)
    return _summarize_events(events[:end])


def _driven_of(events: list[ProxyEvent]) -> EventSlice:
    if not events:
        return _summarize_events([])
    final_idx = len(events) - 1
    loop_start = _loop_start_of(events)
    if loop_start >= final_idx:
        return _summarize_events([])
    return _summarize_events(events[loop_start:final_idx])


def _final_of(events: list[ProxyEvent]) -> EventSlice:
    if not events:
        return _summarize_events([])
    return _summarize_events(events[-1:])


def _detect_turn_starts(events: list[ProxyEvent]) -> list[int]:
    """Indices where a new user turn begins.

    Rule: event 0 always starts turn 0 (if the session has any events).
    For subsequent events, a new turn starts only when BOTH:

    1. The event's request introduces one or more fresh user prompts
       (hash not previously seen this session, filter-passing — see
       :func:`~bnf.proxy.parser.count_fresh_user_prompts`), AND
    2. The previous event's response carried no tool_call (i.e. the
       agent considered itself done and was not awaiting a tool_result).

    This preserves single-prompt sessions as a single turn — subagent
    invocations fail condition (2) because the parent event emitted a
    tool_call; claude-code's auto-title pre-flight fails (1) because
    the fresh prompt set doesn't grow.
    """

    if not events:
        return []
    starts = [0]
    for i in range(1, len(events)):
        prev = events[i - 1]
        curr = events[i]
        prev_finished = prev.content_from_model.tool_call_count == 0
        has_fresh_prompt = curr.fresh_user_prompts > 0
        if has_fresh_prompt and prev_finished:
            starts.append(i)
    return starts


def _compute_harness_gap(prior_events: list[ProxyEvent], started_at: float) -> float:
    """Seconds between the previous event's ``completed_at`` and this
    event's ``started_at``. Zero for the first event in a session."""

    if not prior_events:
        return 0.0
    prev = prior_events[-1]
    return max(0.0, started_at - prev.completed_at)


def _summarize_events(events: list[ProxyEvent]) -> EventSlice:
    slice_ = EventSlice(event_count=len(events))
    for evt in events:
        slice_.raw_request_bytes += evt.raw_request_bytes
        slice_.raw_response_bytes += evt.raw_response_bytes
        slice_.wall_clock_seconds += evt.wall_clock_seconds
        ttft = evt.ttft_seconds
        gen = evt.generation_seconds
        if ttft is not None:
            slice_.ttft_seconds += ttft
        if gen is not None:
            slice_.generation_seconds += gen
        slice_.harness_gap_seconds += evt.harness_gap_seconds
        slice_.content_to_model.add(evt.content_to_model)
        slice_.novel_content_to_model.add(evt.novel_content_to_model)
        slice_.content_from_model.add(evt.content_from_model)
        if evt.usage:
            _add_usage_dict(slice_.vendor, evt.usage)
    return slice_


def _add_usage_dict(counter: VendorCounter, usage: dict[str, int]) -> None:
    counter.total_input_tokens += usage.get("total_input_tokens", 0)
    counter.cached_input_tokens += usage.get("cached_input_tokens", 0)
    counter.uncached_input_tokens += usage.get("uncached_input_tokens", 0)
    counter.cache_creation_input_tokens += usage.get("cache_creation_input_tokens", 0)
    counter.non_reasoning_output_tokens += usage.get("non_reasoning_output_tokens", 0)
    counter.reasoning_output_tokens += usage.get("reasoning_output_tokens", 0)
    counter.total_tokens += usage.get("total_tokens", 0)


@dataclass
class _SessionState:
    id: str
    upstream: str
    upstream_base: str
    requests: int = 0
    raw_request_bytes: int = 0
    raw_response_bytes: int = 0
    first_request_at: Optional[float] = None
    last_request_at: Optional[float] = None
    vendor: VendorCounter = field(default_factory=VendorCounter)
    content_to_model: ContentBreakdown = field(default_factory=ContentBreakdown)
    novel_content_to_model: ContentBreakdown = field(default_factory=ContentBreakdown)
    content_from_model: ContentBreakdown = field(default_factory=ContentBreakdown)
    events: list[ProxyEvent] = field(default_factory=list)
    novel_tracker: NovelTracker = field(default_factory=NovelTracker)
    # Set of user-prompt-text hashes seen so far; drives multi-turn
    # detection. Separate from NovelTracker because it filters harness
    # injections like ``<system-reminder>``.
    seen_user_prompt_hashes: set[str] = field(default_factory=set)
    lock: threading.Lock = field(default_factory=threading.Lock)


class Gateway:
    """Lifecycle owner for the proxy server and its sessions."""

    def __init__(
        self,
        host: str = "127.0.0.1",
        port: int = 0,
        dump_dir: Optional[str] = None,
    ) -> None:
        self._host = host
        self._requested_port = port
        self._sessions: dict[str, _SessionState] = {}
        self._sessions_lock = threading.Lock()
        self._thread: Optional[threading.Thread] = None
        self._loop: Optional[asyncio.AbstractEventLoop] = None
        self._runner: Optional[web.AppRunner] = None
        self._site: Optional[web.BaseSite] = None
        self._client: Optional[aiohttp.ClientSession] = None
        self._started_evt = threading.Event()
        self._start_error: Optional[BaseException] = None
        self._actual_port: int = 0
        # If set, raw request/response bodies are written to
        # ``{dump_dir}/{session_id}/{event_index}.{req,resp}``. Intended for
        # debugging unfamiliar vendor responses; skip for production runs.
        self._dump_dir = dump_dir

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    @classmethod
    def start(
        cls,
        host: str = "127.0.0.1",
        port: int = 0,
        dump_dir: Optional[str] = None,
    ) -> "Gateway":
        gw = cls(host=host, port=port, dump_dir=dump_dir)
        gw._start()
        return gw

    @property
    def base_url(self) -> str:
        if self._actual_port == 0:
            raise RuntimeError("gateway not started")
        return f"http://{self._host}:{self._actual_port}"

    def _start(self) -> None:
        # Pre-bind the listen socket so the kernel-assigned port is known
        # before the event loop thread reports ready.
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            sock.bind((self._host, self._requested_port))
        except OSError:
            sock.close()
            raise
        sock.listen(128)
        sock.setblocking(False)
        self._actual_port = sock.getsockname()[1]
        self._thread = threading.Thread(
            target=self._thread_main,
            args=(sock,),
            daemon=True,
            name="bnf-proxy",
        )
        self._thread.start()
        self._started_evt.wait(timeout=15)
        if self._start_error is not None:
            err = self._start_error
            self._start_error = None
            raise err
        if not self._started_evt.is_set():
            raise RuntimeError("gateway failed to start within 15s")

    def _thread_main(self, sock: socket.socket) -> None:
        loop = asyncio.new_event_loop()
        self._loop = loop
        asyncio.set_event_loop(loop)
        try:
            loop.run_until_complete(self._async_startup(sock))
        except BaseException as exc:
            self._start_error = exc
            try:
                sock.close()
            except OSError:
                pass
            self._started_evt.set()
            loop.close()
            return
        self._started_evt.set()
        try:
            loop.run_forever()
        finally:
            try:
                loop.run_until_complete(self._async_cleanup())
            except Exception:
                pass
            loop.close()

    async def _async_startup(self, sock: socket.socket) -> None:
        self._client = aiohttp.ClientSession(
            timeout=aiohttp.ClientTimeout(total=None, sock_connect=30, sock_read=None),
        )
        app = web.Application(client_max_size=1024 * 1024 * 1024)
        app.router.add_route("*", "/s/{sid}/{tail:.*}", self._handle)
        self._runner = web.AppRunner(app, access_log=None)
        await self._runner.setup()
        self._site = web.SockSite(self._runner, sock)
        await self._site.start()

    async def _async_cleanup(self) -> None:
        if self._site is not None:
            try:
                await self._site.stop()
            except Exception:
                pass
        if self._runner is not None:
            try:
                await self._runner.cleanup()
            except Exception:
                pass
        if self._client is not None:
            try:
                await self._client.close()
            except Exception:
                pass

    def stop(self) -> None:
        loop = self._loop
        if loop is None or not loop.is_running():
            return
        loop.call_soon_threadsafe(loop.stop)
        thread = self._thread
        if thread is not None:
            thread.join(timeout=10)

    def __enter__(self) -> "Gateway":
        if not self._started_evt.is_set():
            self._start()
        return self

    def __exit__(self, *_exc: object) -> None:
        self.stop()

    # ------------------------------------------------------------------
    # Session management
    # ------------------------------------------------------------------

    def create_session(
        self,
        *,
        upstream: str,
        session_id: Optional[str] = None,
        upstream_base: Optional[str] = None,
    ) -> SessionHandle:
        """Register a tracked session and return its handle.

        ``upstream`` must be one of :data:`UPSTREAMS`. ``upstream_base`` lets
        tests point a session at a local mock server instead of the real
        vendor API while still exercising the parsing path chosen by
        ``upstream``.
        """

        if upstream not in UPSTREAMS:
            raise ValueError(f"unknown upstream {upstream!r}; expected one of {list(UPSTREAMS)}")
        base = (upstream_base or UPSTREAMS[upstream]).rstrip("/")
        sid = session_id or secrets.token_urlsafe(12)
        state = _SessionState(id=sid, upstream=upstream, upstream_base=base)
        with self._sessions_lock:
            if sid in self._sessions:
                raise ValueError(f"session {sid!r} already exists")
            self._sessions[sid] = state
        return SessionHandle(
            id=sid,
            base_url=f"{self.base_url}/s/{sid}",
            upstream=upstream,
        )

    def get_stats(self, session_id: str) -> SessionStats:
        with self._sessions_lock:
            state = self._sessions.get(session_id)
        if state is None:
            raise KeyError(session_id)
        with state.lock:
            wall = 0.0
            if state.first_request_at is not None and state.last_request_at is not None:
                wall = state.last_request_at - state.first_request_at
            return SessionStats(
                id=state.id,
                upstream=state.upstream,
                base_url=f"{self.base_url}/s/{state.id}",
                requests=state.requests,
                raw_request_bytes=state.raw_request_bytes,
                raw_response_bytes=state.raw_response_bytes,
                first_request_at=state.first_request_at,
                last_request_at=state.last_request_at,
                wall_clock_seconds=wall,
                vendor=dataclasses.replace(state.vendor),
                content_to_model=state.content_to_model.clone(),
                novel_content_to_model=state.novel_content_to_model.clone(),
                content_from_model=state.content_from_model.clone(),
                events=list(state.events),
            )

    def close_session(self, session_id: str) -> SessionStats:
        stats = self.get_stats(session_id)
        with self._sessions_lock:
            self._sessions.pop(session_id, None)
        return stats

    # ------------------------------------------------------------------
    # Request handler
    # ------------------------------------------------------------------

    async def _handle(self, request: web.Request) -> web.StreamResponse:
        sid = request.match_info["sid"]
        tail = request.match_info["tail"]
        with self._sessions_lock:
            state = self._sessions.get(sid)
        if state is None:
            return web.json_response(
                {"error": {"type": "proxy_error", "message": f"unknown session {sid!r}"}},
                status=404,
            )
        return await self._proxy_to_upstream(request, state, tail)

    async def _proxy_to_upstream(
        self,
        request: web.Request,
        state: _SessionState,
        tail: str,
    ) -> web.StreamResponse:
        client = self._client
        assert client is not None

        upstream_url = f"{state.upstream_base}/{tail}"
        if request.query_string:
            upstream_url = f"{upstream_url}?{request.query_string}"

        fwd_headers = {
            k: v for k, v in request.headers.items() if k.lower() not in _SKIP_REQUEST_HEADERS
        }

        body = await request.read()
        started = time.time()
        parser_family = _PARSER_FAMILY[state.upstream]
        request_content = analyze_request_body(body, parser_family)
        with state.lock:
            # Novel dedup + fresh-prompt detection happen under the
            # session lock so concurrent requests can't race on the
            # trackers.
            novel_request_content = analyze_request_body_novel(
                body, parser_family, state.novel_tracker
            )
            fresh_prompts = count_fresh_user_prompts(
                body, parser_family, state.seen_user_prompt_hashes
            )
            state.requests += 1
            state.raw_request_bytes += len(body)
            state.content_to_model.add(request_content)
            state.novel_content_to_model.add(novel_request_content)
            if state.first_request_at is None:
                state.first_request_at = started
            state.last_request_at = started
        # ``event.index`` is assigned at append time so concurrent requests
        # don't collide on the same value.

        try:
            upstream_resp = await client.request(
                request.method,
                upstream_url,
                headers=fwd_headers,
                data=body if body else None,
                allow_redirects=False,
            )
        except Exception as exc:
            completed = time.time()
            event = ProxyEvent(
                index=-1,
                method=request.method,
                path="/" + tail,
                status=502,
                raw_request_bytes=len(body),
                raw_response_bytes=0,
                started_at=started,
                completed_at=completed,
                streaming=False,
                content_to_model=request_content.clone(),
                novel_content_to_model=novel_request_content.clone(),
                fresh_user_prompts=fresh_prompts,
                error=f"{type(exc).__name__}: {exc}",
            )
            with state.lock:
                event.index = len(state.events)
                event.harness_gap_seconds = _compute_harness_gap(state.events, started)
                state.events.append(event)
                state.last_request_at = completed
            return web.json_response(
                {"error": {"type": "proxy_upstream_error", "message": str(exc)}},
                status=502,
            )

        try:
            resp_headers = {
                k: v
                for k, v in upstream_resp.headers.items()
                if k.lower() not in _SKIP_RESPONSE_HEADERS
            }
            content_type = upstream_resp.headers.get("content-type", "")
            is_sse = "text/event-stream" in content_type
            is_json = "application/json" in content_type
            # ChatGPT backend omits Content-Type on the Responses endpoint;
            # sniff the first chunk instead.
            needs_sniff = not is_sse and not is_json

            response = web.StreamResponse(status=upstream_resp.status, headers=resp_headers)
            if is_sse:
                # Force chunked framing so the client receives events live.
                response.enable_chunked_encoding()
            await response.prepare(request)

            stream_parser: Optional[StreamUsageParser] = None
            stream_content: Optional[StreamContentAnalyzer] = None
            json_buffer: Optional[bytearray] = None
            dump_buffer: Optional[bytearray] = None
            if is_sse:
                stream_parser = StreamUsageParser(parser_family)
                stream_content = StreamContentAnalyzer(parser_family)
            elif is_json:
                json_buffer = bytearray()
            if self._dump_dir and json_buffer is None:
                dump_buffer = bytearray()

            total_bytes = 0
            first_response_byte_at: Optional[float] = None
            try:
                async for chunk in upstream_resp.content.iter_any():
                    if not chunk:
                        continue
                    if first_response_byte_at is None:
                        first_response_byte_at = time.time()
                    if needs_sniff:
                        head = chunk.lstrip()[:16]
                        if head.startswith((b"event:", b"data:", b":")):
                            is_sse = True
                            stream_parser = StreamUsageParser(parser_family)
                            stream_content = StreamContentAnalyzer(parser_family)
                        elif head.startswith(b"{"):
                            is_json = True
                            if json_buffer is None:
                                json_buffer = bytearray()
                        needs_sniff = False
                    total_bytes += len(chunk)
                    await response.write(chunk)
                    if stream_parser is not None:
                        stream_parser.feed(chunk)
                    if stream_content is not None:
                        stream_content.feed(chunk)
                    if json_buffer is not None:
                        json_buffer.extend(chunk)
                    if dump_buffer is not None:
                        dump_buffer.extend(chunk)
            except Exception as exc:
                completed = time.time()
                partial_content = (
                    stream_content.breakdown.clone()
                    if stream_content is not None
                    else ContentBreakdown()
                )
                event = ProxyEvent(
                    index=-1,
                    method=request.method,
                    path="/" + tail,
                    status=upstream_resp.status,
                    raw_request_bytes=len(body),
                    raw_response_bytes=total_bytes,
                    started_at=started,
                    completed_at=completed,
                    streaming=is_sse,
                    content_type=content_type,
                    first_response_byte_at=first_response_byte_at,
                    content_to_model=request_content.clone(),
                    novel_content_to_model=novel_request_content.clone(),
                    fresh_user_prompts=fresh_prompts,
                    content_from_model=partial_content,
                    error=f"stream_interrupted: {type(exc).__name__}: {exc}",
                )
                with state.lock:
                    state.raw_response_bytes += total_bytes
                    state.content_from_model.add(partial_content)
                    state.last_request_at = completed
                    event.index = len(state.events)
                    event.harness_gap_seconds = _compute_harness_gap(state.events, started)
                    state.events.append(event)
                return response

            # ``write_eof`` can raise ClientConnectionResetError if the
            # client already closed — we still have the full upstream body
            # and usage, so record the event before propagating.
            write_eof_error: Optional[str] = None
            try:
                await response.write_eof()
            except Exception as exc:
                write_eof_error = f"client_closed: {type(exc).__name__}: {exc}"

            usage: Optional[UsageSnapshot] = None
            response_content = ContentBreakdown()
            if stream_parser is not None:
                stream_parser.finalize()
                if not stream_parser.snapshot.is_empty():
                    usage = stream_parser.snapshot
            if stream_content is not None:
                stream_content.finalize()
                response_content = stream_content.breakdown.clone()
            if json_buffer is not None:
                payload = bytes(json_buffer)
                usage = parse_json_usage(payload, parser_family) or usage
                response_content = analyze_response_body(payload, parser_family)

            completed = time.time()
            event = ProxyEvent(
                index=-1,
                method=request.method,
                path="/" + tail,
                status=upstream_resp.status,
                raw_request_bytes=len(body),
                raw_response_bytes=total_bytes,
                started_at=started,
                completed_at=completed,
                streaming=is_sse,
                content_type=content_type,
                first_response_byte_at=first_response_byte_at,
                content_to_model=request_content.clone(),
                novel_content_to_model=novel_request_content.clone(),
                fresh_user_prompts=fresh_prompts,
                content_from_model=response_content.clone(),
                usage=usage.to_dict() if usage is not None else None,
                error=write_eof_error,
            )
            with state.lock:
                state.raw_response_bytes += total_bytes
                state.content_from_model.add(response_content)
                state.last_request_at = completed
                event.index = len(state.events)
                event.harness_gap_seconds = _compute_harness_gap(state.events, started)
                state.events.append(event)
                if usage is not None:
                    state.vendor.add(usage)

            if self._dump_dir:
                try:
                    import pathlib

                    d = pathlib.Path(self._dump_dir) / state.id
                    d.mkdir(parents=True, exist_ok=True)
                    (d / f"{event.index:04d}.req").write_bytes(body)
                    payload = (
                        bytes(json_buffer)
                        if json_buffer is not None
                        else (bytes(dump_buffer) if dump_buffer is not None else b"")
                    )
                    (d / f"{event.index:04d}.resp").write_bytes(payload)
                    (d / f"{event.index:04d}.meta").write_text(
                        f"{request.method} /{tail}\nstatus={upstream_resp.status}\n"
                        f"content-type={content_type}\n",
                        encoding="utf-8",
                    )
                except Exception:
                    pass
            return response
        finally:
            upstream_resp.release()
