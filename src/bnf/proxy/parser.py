"""Usage + content parsing for Anthropic and OpenAI API responses.

This module produces two kinds of measurements:

1. :class:`UsageSnapshot` — vendor-reported tokens, split into
   ``non_reasoning_output_tokens`` vs ``reasoning_output_tokens`` for cross-
   vendor clarity. Anthropic does not expose a native non-reasoning /
   reasoning split in ``usage``; for Anthropic responses the whole
   ``output_tokens`` lands in ``non_reasoning_output_tokens`` (we still
   capture the reasoning bytes separately in :class:`ContentBreakdown`).
2. :class:`ContentBreakdown` — bytes of the actual conversational /
   instructional payload, bucketed by role so they are comparable across
   vendors. Protocol framing (JSON brackets, field names, model params,
   HTTP headers) is deliberately excluded; only UTF-8 text leaves are
   counted.

See :class:`StreamUsageParser` / :class:`StreamContentAnalyzer` for
incremental SSE parsing, and :func:`parse_json_usage` /
:func:`analyze_request_body` / :func:`analyze_response_body` for one-shot
JSON parsing.
"""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass, field, fields
from typing import Any, Optional


# ---------------------------------------------------------------------------
# Vendor usage (tokens)
# ---------------------------------------------------------------------------


@dataclass
class UsageSnapshot:
    """Per-request vendor usage, normalized for cross-vendor comparability.

    Anthropic and OpenAI disagree on what ``input_tokens`` means:

    * Anthropic's ``input_tokens`` is the *fresh, not-being-cached* slice
      of input. Tokens served from cache live in
      ``cache_read_input_tokens``; tokens being *written* to cache live in
      ``cache_creation_input_tokens``. The three are additive.
    * OpenAI's ``input_tokens`` is the *total* input, and
      ``input_tokens_details.cached_tokens`` is a *subset* of that total.

    Blindly comparing those values misleads by orders of magnitude. The
    fields below restate both vendors in one shape:

    * ``total_input_tokens`` — everything the model saw as input this turn.
    * ``cached_input_tokens`` — slice served from cache.
    * ``uncached_input_tokens`` — slice NOT served from cache
      (= ``total_input_tokens - cached_input_tokens``).
    * ``cache_creation_input_tokens`` — Anthropic-only sub-slice of
      ``uncached`` being *written* to cache this turn; ``0`` for OpenAI
      (no separate reporting).
    * ``non_reasoning_output_tokens`` / ``reasoning_output_tokens`` —
      output split; avoids the ambiguous ``output_tokens``.
    * ``total_tokens`` — everything the model billed for.
    """

    total_input_tokens: int = 0
    cached_input_tokens: int = 0
    uncached_input_tokens: int = 0
    cache_creation_input_tokens: int = 0
    non_reasoning_output_tokens: int = 0
    reasoning_output_tokens: int = 0
    total_tokens: int = 0

    def is_empty(self) -> bool:
        return all(getattr(self, f.name) == 0 for f in fields(self))

    def to_dict(self) -> dict[str, int]:
        return {f.name: getattr(self, f.name) for f in fields(self)}


# ---------------------------------------------------------------------------
# Conversational content (bytes of actual text)
# ---------------------------------------------------------------------------


@dataclass
class ContentBreakdown:
    """Bytes of UTF-8 text payload + item counts, bucketed for
    cross-vendor comparability.

    Conversational buckets (comparable across vendors):

    * ``user_text_bytes`` — user-role text
    * ``tool_result_bytes`` — tool output fed back into the model
    * ``assistant_text_bytes`` — assistant-visible text (excluding tool_use)
    * ``tool_call_bytes`` — arguments of tool_use / function_call
    * ``reasoning_bytes`` — chain-of-thought / thinking / reasoning content

    Per-turn overhead (not directly comparable — each harness ships its
    own system prompt and tool schema, often very different sizes):

    * ``system_prompt_bytes``
    * ``tool_schema_bytes``

    Item counts (comparable across vendors):

    * ``tool_call_count`` — number of tool_use / function_call items in
      this payload
    * ``tool_result_count`` — number of tool_result / function_call_output
      items in this payload

    All byte fields count only UTF-8 text leaves; JSON framing, field
    names, model parameters, and transport metadata are excluded.

    At session level, breakdowns accumulated from wire responses contain
    every tool_call exactly once (responses are never replayed), so
    ``content_from_model.tool_call_count`` is the true number of tool
    calls the model made. By contrast, accumulating per-event request
    breakdowns will inflate tool_result_count because each request
    replays the full conversation history; see
    :class:`NovelTracker` for a deduplicated view.
    """

    user_text_bytes: int = 0
    tool_result_bytes: int = 0
    assistant_text_bytes: int = 0
    tool_call_bytes: int = 0
    reasoning_bytes: int = 0
    system_prompt_bytes: int = 0
    tool_schema_bytes: int = 0
    tool_call_count: int = 0
    tool_result_count: int = 0
    #: Frequency of tool names invoked (``{"Read": 3, "Grep": 1}``). In
    #: response breakdowns this is the model's tool usage; in request
    #: breakdowns it's the replayed history.
    tool_names: dict[str, int] = field(default_factory=dict)

    def add(self, other: "ContentBreakdown") -> None:
        self.user_text_bytes += other.user_text_bytes
        self.tool_result_bytes += other.tool_result_bytes
        self.assistant_text_bytes += other.assistant_text_bytes
        self.tool_call_bytes += other.tool_call_bytes
        self.reasoning_bytes += other.reasoning_bytes
        self.system_prompt_bytes += other.system_prompt_bytes
        self.tool_schema_bytes += other.tool_schema_bytes
        self.tool_call_count += other.tool_call_count
        self.tool_result_count += other.tool_result_count
        for name, count in other.tool_names.items():
            self.tool_names[name] = self.tool_names.get(name, 0) + count

    def clone(self) -> "ContentBreakdown":
        """Return a deep-enough copy that downstream mutations to
        ``tool_names`` don't leak into the source."""

        return ContentBreakdown(
            user_text_bytes=self.user_text_bytes,
            tool_result_bytes=self.tool_result_bytes,
            assistant_text_bytes=self.assistant_text_bytes,
            tool_call_bytes=self.tool_call_bytes,
            reasoning_bytes=self.reasoning_bytes,
            system_prompt_bytes=self.system_prompt_bytes,
            tool_schema_bytes=self.tool_schema_bytes,
            tool_call_count=self.tool_call_count,
            tool_result_count=self.tool_result_count,
            tool_names=dict(self.tool_names),
        )

    def to_dict(self) -> dict[str, Any]:
        out: dict[str, Any] = {}
        for f in fields(self):
            val = getattr(self, f.name)
            if isinstance(val, dict):
                out[f.name] = dict(val)
            else:
                out[f.name] = val
        return out

    @property
    def conversational_bytes(self) -> int:
        """Sum of buckets that should be comparable across vendors."""
        return (
            self.user_text_bytes
            + self.tool_result_bytes
            + self.assistant_text_bytes
            + self.tool_call_bytes
            + self.reasoning_bytes
        )

    @property
    def overhead_bytes(self) -> int:
        return self.system_prompt_bytes + self.tool_schema_bytes


# ---------------------------------------------------------------------------
# Stream parser — usage only
# ---------------------------------------------------------------------------


class _SSEBase:
    """Shared SSE byte-stream framing. Subclasses implement ``_apply_event``."""

    def __init__(self) -> None:
        self._buf = bytearray()

    def feed(self, chunk: bytes) -> None:
        if not chunk:
            return
        self._buf.extend(chunk.replace(b"\r\n", b"\n").replace(b"\r", b"\n"))
        while True:
            idx = self._buf.find(b"\n\n")
            if idx < 0:
                return
            raw = bytes(self._buf[:idx])
            del self._buf[: idx + 2]
            self._dispatch(raw)

    def finalize(self) -> None:
        remaining = bytes(self._buf).strip()
        self._buf.clear()
        if remaining:
            self._dispatch(remaining)

    def _dispatch(self, raw: bytes) -> None:
        data_parts: list[str] = []
        for line in raw.split(b"\n"):
            if line.startswith(b"data:"):
                payload = line[5:]
                if payload.startswith(b" "):
                    payload = payload[1:]
                data_parts.append(payload.decode("utf-8", errors="replace"))
        if not data_parts:
            return
        data = "\n".join(data_parts).strip()
        if not data or data == "[DONE]":
            return
        try:
            obj = json.loads(data)
        except json.JSONDecodeError:
            return
        if isinstance(obj, dict):
            self._apply_event(obj)

    def _apply_event(self, obj: dict[str, Any]) -> None:  # pragma: no cover - abstract
        raise NotImplementedError


class StreamUsageParser(_SSEBase):
    """Accumulates :class:`UsageSnapshot` from an SSE byte stream."""

    def __init__(self, upstream: str) -> None:
        super().__init__()
        self.upstream = upstream
        self.snapshot = UsageSnapshot()

    def _apply_event(self, obj: dict[str, Any]) -> None:
        etype = obj.get("type")
        if etype == "message_start":
            msg = obj.get("message")
            if isinstance(msg, dict):
                _merge_anthropic_usage(self.snapshot, msg.get("usage"))
        elif etype == "message_delta":
            _merge_anthropic_usage(self.snapshot, obj.get("usage"))
        elif etype == "response.completed":
            resp = obj.get("response")
            if isinstance(resp, dict):
                _merge_openai_usage(self.snapshot, resp.get("usage"))


# ---------------------------------------------------------------------------
# Stream analyzer — content bytes
# ---------------------------------------------------------------------------


class StreamContentAnalyzer(_SSEBase):
    """Accumulates a :class:`ContentBreakdown` from the RESPONSE stream.

    Bytes are taken from delta payloads only; none of the event framing or
    JSON keys are counted.
    """

    def __init__(self, upstream: str) -> None:
        super().__init__()
        self.upstream = upstream
        self.breakdown = ContentBreakdown()
        # Anthropic: index → block type ("text" | "tool_use" | "thinking")
        self._anthropic_blocks: dict[int, str] = {}

    def _apply_event(self, obj: dict[str, Any]) -> None:
        etype = obj.get("type")
        if self.upstream == "anthropic":
            self._apply_anthropic(etype, obj)
        else:
            self._apply_openai(etype, obj)

    # -- Anthropic -------------------------------------------------------

    def _apply_anthropic(self, etype: Any, obj: dict[str, Any]) -> None:
        if etype == "content_block_start":
            idx = obj.get("index")
            block = obj.get("content_block")
            if isinstance(block, dict) and isinstance(idx, int):
                btype = block.get("type")
                if isinstance(btype, str):
                    self._anthropic_blocks[idx] = btype
                # Initial content that already shipped with the block.
                if btype == "text":
                    self.breakdown.assistant_text_bytes += _utf8_len(block.get("text"))
                elif btype == "tool_use":
                    self.breakdown.tool_call_bytes += _string_leaf_bytes(block.get("input"))
                    self.breakdown.tool_call_count += 1
                    _bump_tool_name(self.breakdown, block.get("name"))
                elif btype == "thinking":
                    self.breakdown.reasoning_bytes += _utf8_len(block.get("thinking"))
        elif etype == "content_block_delta":
            idx = obj.get("index")
            delta = obj.get("delta")
            if not isinstance(delta, dict) or not isinstance(idx, int):
                return
            btype = self._anthropic_blocks.get(idx, "")
            dtype = delta.get("type")
            if btype == "text" and dtype == "text_delta":
                self.breakdown.assistant_text_bytes += _utf8_len(delta.get("text"))
            elif btype == "tool_use" and dtype == "input_json_delta":
                self.breakdown.tool_call_bytes += _utf8_len(delta.get("partial_json"))
            elif btype == "thinking" and dtype == "thinking_delta":
                self.breakdown.reasoning_bytes += _utf8_len(delta.get("thinking"))

    # -- OpenAI Responses -----------------------------------------------

    def _apply_openai(self, etype: Any, obj: dict[str, Any]) -> None:
        if etype == "response.output_item.added":
            item = obj.get("item")
            if isinstance(item, dict) and item.get("type") == "function_call":
                # Count once per emitted function_call, regardless of how many
                # argument-delta events follow.
                self.breakdown.tool_call_count += 1
                _bump_tool_name(self.breakdown, item.get("name"))
        elif etype == "response.output_text.delta":
            self.breakdown.assistant_text_bytes += _utf8_len(obj.get("delta"))
        elif etype in (
            "response.reasoning_text.delta",
            "response.reasoning_summary_text.delta",
        ):
            self.breakdown.reasoning_bytes += _utf8_len(obj.get("delta"))
        elif etype == "response.function_call_arguments.delta":
            self.breakdown.tool_call_bytes += _utf8_len(obj.get("delta"))


# ---------------------------------------------------------------------------
# Session-scoped novelty tracking
# ---------------------------------------------------------------------------


@dataclass
class NovelTracker:
    """Cross-event dedup state for a single session.

    Each request replays the full conversation history, so naive
    aggregation of per-event request breakdowns double-counts tool
    results, tool_use echoes, system prompts, and tool schemas.
    :func:`analyze_request_body_novel` consults and updates this
    tracker to return only the slice of the request that is
    first-appearing in the session — i.e. the novel bytes contributed
    by this turn.

    Blocks with stable IDs (``tool_use.id``, ``tool_result.tool_use_id``
    / ``function_call.call_id``, ``function_call_output.call_id``) are
    deduped by ID. Text / thinking / system / tool_schema blocks are
    deduped by content hash — an approximation that is correct when
    harness content is stable turn-to-turn (the common case) and
    treats any change as a novel block (conservatively counting more
    than less).
    """

    seen_tool_use_ids: set[str] = field(default_factory=set)
    seen_tool_result_ids: set[str] = field(default_factory=set)
    seen_text_hashes: set[str] = field(default_factory=set)
    seen_thinking_hashes: set[str] = field(default_factory=set)
    seen_system_hashes: set[str] = field(default_factory=set)
    seen_tool_schema_hashes: set[str] = field(default_factory=set)


def _hash(payload: Any) -> str:
    if isinstance(payload, str):
        data = payload.encode("utf-8")
    else:
        data = json.dumps(payload, sort_keys=True, ensure_ascii=False).encode("utf-8")
    return hashlib.sha256(data).hexdigest()


# ---------------------------------------------------------------------------
# Request / response JSON analysis
# ---------------------------------------------------------------------------


def analyze_request_body(body: bytes, upstream: str) -> ContentBreakdown:
    """Parse a request JSON body into a :class:`ContentBreakdown`.

    Returns an empty breakdown if the body is not valid JSON. Counts only
    UTF-8 text leaves; JSON framing and numeric/boolean parameters are
    ignored.
    """

    br = ContentBreakdown()
    if not body:
        return br
    try:
        obj = json.loads(body)
    except (json.JSONDecodeError, UnicodeDecodeError):
        return br
    if not isinstance(obj, dict):
        return br
    if upstream == "anthropic":
        _analyze_anthropic_request(obj, br)
    else:
        _analyze_openai_request(obj, br)
    return br


def count_fresh_user_prompts(body: bytes, upstream: str, seen_hashes: set[str]) -> int:
    """Return the number of user-authored prompts in this request that
    haven't been seen before in ``seen_hashes``. The set is mutated in
    place.

    Harness-injected user-role text blocks (Claude-Code's
    ``<system-reminder>`` / ``<command-name>`` wrappers) are filtered
    out so they don't trick multi-turn detection into over-counting.
    Tool results and non-text content never count.
    """

    if not body:
        return 0
    try:
        obj = json.loads(body)
    except (json.JSONDecodeError, UnicodeDecodeError):
        return 0
    if not isinstance(obj, dict):
        return 0
    if upstream == "anthropic":
        return _count_fresh_anthropic_prompts(obj, seen_hashes)
    return _count_fresh_openai_prompts(obj, seen_hashes)


def _count_fresh_anthropic_prompts(obj: dict[str, Any], seen_hashes: set[str]) -> int:
    count = 0
    for msg in obj.get("messages") or []:
        if not isinstance(msg, dict) or msg.get("role") != "user":
            continue
        content = msg.get("content")
        if isinstance(content, str):
            if _is_user_prompt_text(content):
                count += _register_prompt(content, seen_hashes)
            continue
        if not isinstance(content, list):
            continue
        for block in content:
            if not isinstance(block, dict):
                continue
            if block.get("type") != "text":
                continue
            text = block.get("text")
            if _is_user_prompt_text(text):
                count += _register_prompt(text, seen_hashes)
    return count


def _count_fresh_openai_prompts(obj: dict[str, Any], seen_hashes: set[str]) -> int:
    count = 0
    input_items = obj.get("input")
    if isinstance(input_items, str):
        if _is_user_prompt_text(input_items):
            count += _register_prompt(input_items, seen_hashes)
        return count
    if not isinstance(input_items, list):
        return count
    for item in input_items:
        if not isinstance(item, dict):
            continue
        itype = item.get("type")
        role = item.get("role")
        # Only plain user messages count; function_call_output and
        # reasoning items never. Developer / system roles are harness
        # instructions, not user prompts.
        if role != "user":
            continue
        if itype not in (None, "message"):
            continue
        content = item.get("content")
        if isinstance(content, str):
            if _is_user_prompt_text(content):
                count += _register_prompt(content, seen_hashes)
            continue
        if not isinstance(content, list):
            continue
        for part in content:
            if not isinstance(part, dict):
                continue
            ptype = part.get("type")
            if ptype not in ("input_text", "text"):
                continue
            text = part.get("text")
            if _is_user_prompt_text(text):
                count += _register_prompt(text, seen_hashes)
    return count


def _register_prompt(text: str, seen_hashes: set[str]) -> int:
    h = _hash(text)
    if h in seen_hashes:
        return 0
    seen_hashes.add(h)
    return 1


def _is_user_prompt_text(text: Any) -> bool:
    """Accept only plausibly user-authored text. Rejects empty / harness-
    injected wrappers that claude-code sends in the user role."""

    if not isinstance(text, str):
        return False
    stripped = text.strip()
    if not stripped:
        return False
    if stripped.startswith("<system-reminder>"):
        return False
    if stripped.startswith("<command-name>"):
        return False
    return True


def analyze_request_body_novel(
    body: bytes, upstream: str, tracker: NovelTracker
) -> ContentBreakdown:
    """Return only the slice of the request body that is first-appearing
    in the session (per ``tracker``). The tracker is mutated in place to
    record the blocks encountered, so a subsequent call on a replayed
    body returns zero for anything already seen.
    """

    br = ContentBreakdown()
    if not body:
        return br
    try:
        obj = json.loads(body)
    except (json.JSONDecodeError, UnicodeDecodeError):
        return br
    if not isinstance(obj, dict):
        return br
    if upstream == "anthropic":
        _analyze_anthropic_request_novel(obj, br, tracker)
    else:
        _analyze_openai_request_novel(obj, br, tracker)
    return br


def analyze_response_body(body: bytes, upstream: str) -> ContentBreakdown:
    """Parse a non-streaming response JSON body into a breakdown."""

    br = ContentBreakdown()
    if not body:
        return br
    try:
        obj = json.loads(body)
    except (json.JSONDecodeError, UnicodeDecodeError):
        return br
    if not isinstance(obj, dict):
        return br
    if upstream == "anthropic":
        _analyze_anthropic_response(obj, br)
    else:
        _analyze_openai_response(obj, br)
    return br


def _analyze_anthropic_request(obj: dict[str, Any], br: ContentBreakdown) -> None:
    system = obj.get("system")
    if isinstance(system, str):
        br.system_prompt_bytes += _utf8_len(system)
    elif isinstance(system, list):
        for block in system:
            if isinstance(block, dict) and block.get("type") == "text":
                br.system_prompt_bytes += _utf8_len(block.get("text"))
            elif isinstance(block, str):
                br.system_prompt_bytes += _utf8_len(block)

    for msg in obj.get("messages") or []:
        if not isinstance(msg, dict):
            continue
        role = msg.get("role")
        content = msg.get("content")
        if isinstance(content, str):
            if role == "user":
                br.user_text_bytes += _utf8_len(content)
            elif role == "assistant":
                br.assistant_text_bytes += _utf8_len(content)
            continue
        if not isinstance(content, list):
            continue
        for block in content:
            if not isinstance(block, dict):
                continue
            btype = block.get("type")
            if btype == "text":
                t = _utf8_len(block.get("text"))
                if role == "user":
                    br.user_text_bytes += t
                else:
                    br.assistant_text_bytes += t
            elif btype == "tool_result":
                br.tool_result_bytes += _tool_result_bytes(block.get("content"))
                br.tool_result_count += 1
            elif btype == "tool_use":
                # Replayed history of an assistant tool_use.
                br.tool_call_bytes += _string_leaf_bytes(block.get("input"))
                br.tool_call_count += 1
                _bump_tool_name(br, block.get("name"))
            elif btype == "thinking":
                br.reasoning_bytes += _utf8_len(block.get("thinking"))

    tools = obj.get("tools")
    if isinstance(tools, list):
        br.tool_schema_bytes += _string_leaf_bytes(tools)


def _analyze_anthropic_response(obj: dict[str, Any], br: ContentBreakdown) -> None:
    for block in obj.get("content") or []:
        if not isinstance(block, dict):
            continue
        btype = block.get("type")
        if btype == "text":
            br.assistant_text_bytes += _utf8_len(block.get("text"))
        elif btype == "tool_use":
            br.tool_call_bytes += _string_leaf_bytes(block.get("input"))
            br.tool_call_count += 1
            _bump_tool_name(br, block.get("name"))
        elif btype == "thinking":
            br.reasoning_bytes += _utf8_len(block.get("thinking"))


def _analyze_openai_request(obj: dict[str, Any], br: ContentBreakdown) -> None:
    instructions = obj.get("instructions")
    if isinstance(instructions, str):
        br.system_prompt_bytes += _utf8_len(instructions)

    input_items = obj.get("input")
    if isinstance(input_items, str):
        br.user_text_bytes += _utf8_len(input_items)
    elif isinstance(input_items, list):
        for item in input_items:
            if not isinstance(item, dict):
                continue
            itype = item.get("type")
            role = item.get("role")
            if itype == "message" or (itype is None and role):
                _count_openai_message_content(item.get("content"), role, br)
            elif itype == "function_call":
                br.tool_call_bytes += _string_leaf_bytes(item.get("arguments"))
                br.tool_call_count += 1
                _bump_tool_name(br, item.get("name"))
            elif itype == "function_call_output":
                br.tool_result_bytes += _string_leaf_bytes(item.get("output"))
                br.tool_result_count += 1
            elif itype in ("reasoning", "reasoning_text", "reasoning_summary"):
                br.reasoning_bytes += _reasoning_text_bytes(item)

    tools = obj.get("tools")
    if isinstance(tools, list):
        br.tool_schema_bytes += _string_leaf_bytes(tools)


def _count_openai_message_content(content: Any, role: Any, br: ContentBreakdown) -> None:
    if isinstance(content, str):
        _credit_openai_text(br, role, "input_text", content)
        return
    if not isinstance(content, list):
        return
    for part in content:
        if not isinstance(part, dict):
            continue
        ptype = part.get("type")
        text = part.get("text")
        if isinstance(text, str):
            _credit_openai_text(br, role, ptype, text)


def _credit_openai_text(br: ContentBreakdown, role: Any, ptype: Any, text: str) -> None:
    """Attribute an OpenAI message text part to the right bucket.

    OpenAI exposes ``developer`` and ``system`` roles for harness-side
    instructions; those land in ``system_prompt_bytes`` (same bucket as
    the top-level ``instructions`` field) rather than user_text.
    """

    n = _utf8_len(text)
    if ptype == "output_text" or role == "assistant":
        br.assistant_text_bytes += n
    elif role in ("system", "developer"):
        br.system_prompt_bytes += n
    else:
        br.user_text_bytes += n


def _analyze_openai_response(obj: dict[str, Any], br: ContentBreakdown) -> None:
    output = obj.get("output")
    if not isinstance(output, list):
        return
    for item in output:
        if not isinstance(item, dict):
            continue
        itype = item.get("type")
        if itype == "message":
            for part in item.get("content") or []:
                if not isinstance(part, dict):
                    continue
                ptype = part.get("type")
                if ptype == "output_text":
                    br.assistant_text_bytes += _utf8_len(part.get("text"))
        elif itype == "function_call":
            br.tool_call_bytes += _string_leaf_bytes(item.get("arguments"))
            br.tool_call_count += 1
            _bump_tool_name(br, item.get("name"))
        elif itype in ("reasoning", "reasoning_text", "reasoning_summary"):
            for key in ("summary", "content", "text"):
                br.reasoning_bytes += _string_leaf_bytes(item.get(key))


def _analyze_anthropic_request_novel(
    obj: dict[str, Any], br: ContentBreakdown, tracker: NovelTracker
) -> None:
    system = obj.get("system")
    sys_text = _extract_anthropic_system_text(system)
    if sys_text:
        h = _hash(sys_text)
        if h not in tracker.seen_system_hashes:
            tracker.seen_system_hashes.add(h)
            br.system_prompt_bytes += _utf8_len(sys_text)

    for msg in obj.get("messages") or []:
        if not isinstance(msg, dict):
            continue
        role = msg.get("role")
        content = msg.get("content")
        if isinstance(content, str):
            h = _hash(content)
            if h not in tracker.seen_text_hashes:
                tracker.seen_text_hashes.add(h)
                n = _utf8_len(content)
                if role == "user":
                    br.user_text_bytes += n
                else:
                    br.assistant_text_bytes += n
            continue
        if not isinstance(content, list):
            continue
        for block in content:
            if not isinstance(block, dict):
                continue
            btype = block.get("type")
            if btype == "text":
                text = block.get("text")
                if not isinstance(text, str):
                    continue
                h = _hash(text)
                if h not in tracker.seen_text_hashes:
                    tracker.seen_text_hashes.add(h)
                    n = _utf8_len(text)
                    if role == "user":
                        br.user_text_bytes += n
                    else:
                        br.assistant_text_bytes += n
            elif btype == "tool_result":
                tuid = block.get("tool_use_id")
                if isinstance(tuid, str) and tuid not in tracker.seen_tool_result_ids:
                    tracker.seen_tool_result_ids.add(tuid)
                    br.tool_result_bytes += _tool_result_bytes(block.get("content"))
                    br.tool_result_count += 1
            elif btype == "tool_use":
                tid = block.get("id")
                if isinstance(tid, str) and tid not in tracker.seen_tool_use_ids:
                    tracker.seen_tool_use_ids.add(tid)
                    br.tool_call_bytes += _string_leaf_bytes(block.get("input"))
                    br.tool_call_count += 1
                    _bump_tool_name(br, block.get("name"))
            elif btype == "thinking":
                text = block.get("thinking")
                if not isinstance(text, str):
                    continue
                h = _hash(text)
                if h not in tracker.seen_thinking_hashes:
                    tracker.seen_thinking_hashes.add(h)
                    br.reasoning_bytes += _utf8_len(text)

    tools = obj.get("tools")
    if isinstance(tools, list):
        h = _hash(tools)
        if h not in tracker.seen_tool_schema_hashes:
            tracker.seen_tool_schema_hashes.add(h)
            br.tool_schema_bytes += _string_leaf_bytes(tools)


def _extract_anthropic_system_text(system: Any) -> str:
    if isinstance(system, str):
        return system
    if isinstance(system, list):
        parts: list[str] = []
        for block in system:
            if isinstance(block, dict) and block.get("type") == "text":
                t = block.get("text")
                if isinstance(t, str):
                    parts.append(t)
            elif isinstance(block, str):
                parts.append(block)
        return "\n".join(parts)
    return ""


def _analyze_openai_request_novel(
    obj: dict[str, Any], br: ContentBreakdown, tracker: NovelTracker
) -> None:
    instructions = obj.get("instructions")
    if isinstance(instructions, str):
        h = _hash(instructions)
        if h not in tracker.seen_system_hashes:
            tracker.seen_system_hashes.add(h)
            br.system_prompt_bytes += _utf8_len(instructions)

    input_items = obj.get("input")
    if isinstance(input_items, str):
        h = _hash(input_items)
        if h not in tracker.seen_text_hashes:
            tracker.seen_text_hashes.add(h)
            br.user_text_bytes += _utf8_len(input_items)
    elif isinstance(input_items, list):
        for item in input_items:
            if not isinstance(item, dict):
                continue
            itype = item.get("type")
            role = item.get("role")
            if itype == "message" or (itype is None and role):
                _count_openai_message_content_novel(item.get("content"), role, br, tracker)
            elif itype == "function_call":
                cid = item.get("call_id") or item.get("id")
                if isinstance(cid, str) and cid not in tracker.seen_tool_use_ids:
                    tracker.seen_tool_use_ids.add(cid)
                    br.tool_call_bytes += _string_leaf_bytes(item.get("arguments"))
                    br.tool_call_count += 1
                    _bump_tool_name(br, item.get("name"))
            elif itype == "function_call_output":
                cid = item.get("call_id")
                if isinstance(cid, str) and cid not in tracker.seen_tool_result_ids:
                    tracker.seen_tool_result_ids.add(cid)
                    br.tool_result_bytes += _string_leaf_bytes(item.get("output"))
                    br.tool_result_count += 1
            elif itype in ("reasoning", "reasoning_text", "reasoning_summary"):
                text_bytes = _reasoning_text_bytes(item)
                if text_bytes == 0:
                    continue
                # Hash the extracted text-only content (not the whole item,
                # which carries IDs that can change between replays).
                h = _hash(_reasoning_text_payload(item))
                if h not in tracker.seen_thinking_hashes:
                    tracker.seen_thinking_hashes.add(h)
                    br.reasoning_bytes += text_bytes

    tools = obj.get("tools")
    if isinstance(tools, list):
        h = _hash(tools)
        if h not in tracker.seen_tool_schema_hashes:
            tracker.seen_tool_schema_hashes.add(h)
            br.tool_schema_bytes += _string_leaf_bytes(tools)


def _count_openai_message_content_novel(
    content: Any, role: Any, br: ContentBreakdown, tracker: NovelTracker
) -> None:
    # Dedup uses the same bucketing rules as the wire counterpart so the
    # novel slice is always ≤ the wire slice per field.
    if isinstance(content, str):
        h = _hash(content)
        if h not in tracker.seen_text_hashes:
            tracker.seen_text_hashes.add(h)
            _credit_openai_text(br, role, "input_text", content)
        return
    if not isinstance(content, list):
        return
    for part in content:
        if not isinstance(part, dict):
            continue
        ptype = part.get("type")
        text = part.get("text")
        if not isinstance(text, str):
            continue
        if ptype not in ("input_text", "text", "output_text"):
            continue
        h = _hash(text)
        if h in tracker.seen_text_hashes:
            continue
        tracker.seen_text_hashes.add(h)
        _credit_openai_text(br, role, ptype, text)


def _reasoning_text_payload(item: dict[str, Any]) -> list[str]:
    """Collect text leaves from a reasoning item for stable hashing."""

    parts: list[str] = []
    for key in ("text", "summary"):
        val = item.get(key)
        if isinstance(val, str):
            parts.append(val)
        elif isinstance(val, list):
            for block in val:
                if isinstance(block, dict):
                    t = block.get("text")
                    if isinstance(t, str):
                        parts.append(t)
                elif isinstance(block, str):
                    parts.append(block)
    content = item.get("content")
    if isinstance(content, str):
        parts.append(content)
    elif isinstance(content, list):
        for block in content:
            if isinstance(block, dict):
                t = block.get("text")
                if isinstance(t, str):
                    parts.append(t)
            elif isinstance(block, str):
                parts.append(block)
    return parts


def _reasoning_text_bytes(item: dict[str, Any]) -> int:
    """Sum ``text``/``summary`` text leaves inside a reasoning item.

    Avoids counting type-label strings like ``"reasoning_text"`` that would
    otherwise leak in via :func:`_string_leaf_bytes`.
    """

    total = 0
    for key in ("text", "summary"):
        val = item.get(key)
        if isinstance(val, str):
            total += _utf8_len(val)
        elif isinstance(val, list):
            for block in val:
                if isinstance(block, dict):
                    total += _utf8_len(block.get("text"))
                elif isinstance(block, str):
                    total += _utf8_len(block)
    content = item.get("content")
    if isinstance(content, list):
        for block in content:
            if isinstance(block, dict):
                total += _utf8_len(block.get("text"))
            elif isinstance(block, str):
                total += _utf8_len(block)
    elif isinstance(content, str):
        total += _utf8_len(content)
    return total


def _tool_result_bytes(content: Any) -> int:
    if isinstance(content, str):
        return _utf8_len(content)
    if isinstance(content, list):
        total = 0
        for block in content:
            if isinstance(block, dict) and block.get("type") == "text":
                total += _utf8_len(block.get("text"))
            elif isinstance(block, str):
                total += _utf8_len(block)
        return total
    return 0


# ---------------------------------------------------------------------------
# Non-streaming usage parser
# ---------------------------------------------------------------------------


def parse_json_usage(body: bytes, upstream: str) -> Optional[UsageSnapshot]:
    """Parse usage from a full JSON response body. Returns ``None`` if the
    body is not JSON or carries no ``usage`` field we recognize."""

    if not body:
        return None
    try:
        obj = json.loads(body)
    except (json.JSONDecodeError, UnicodeDecodeError):
        return None
    if not isinstance(obj, dict):
        return None
    usage = obj.get("usage")
    if not isinstance(usage, dict):
        return None
    snap = UsageSnapshot()
    if upstream == "anthropic":
        _merge_anthropic_usage(snap, usage)
    else:
        _merge_openai_usage(snap, usage)
    return snap if not snap.is_empty() else None


def _merge_anthropic_usage(snap: UsageSnapshot, usage: Any) -> None:
    if not isinstance(usage, dict):
        return
    # Anthropic streams partial usage across ``message_start`` and
    # ``message_delta`` — take the max of each field so we survive
    # out-of-order or repeated delivery.
    vendor_input = _as_int(usage.get("input_tokens"))
    cache_creation = _as_int(usage.get("cache_creation_input_tokens"))
    cache_read = _as_int(usage.get("cache_read_input_tokens"))
    # Anthropic's ``input_tokens`` = uncached, not-being-cached. Cache
    # creation is ALSO uncached this turn (just tagged for reuse next
    # time). Comparable "uncached" = input_tokens + cache_creation.
    uncached = vendor_input + cache_creation
    total_input = uncached + cache_read
    snap.cache_creation_input_tokens = max(snap.cache_creation_input_tokens, cache_creation)
    snap.cached_input_tokens = max(snap.cached_input_tokens, cache_read)
    snap.uncached_input_tokens = max(snap.uncached_input_tokens, uncached)
    snap.total_input_tokens = max(snap.total_input_tokens, total_input)
    # Anthropic does not split output into reasoning / non-reasoning in
    # its ``usage`` payload — ``output_tokens`` folds in extended
    # thinking. We park the whole thing in ``non_reasoning`` and rely on
    # :class:`ContentBreakdown` to expose thinking bytes separately when
    # the model streams them.
    snap.non_reasoning_output_tokens = max(
        snap.non_reasoning_output_tokens,
        _as_int(usage.get("output_tokens")),
    )
    snap.total_tokens = (
        snap.total_input_tokens + snap.non_reasoning_output_tokens + snap.reasoning_output_tokens
    )


def _merge_openai_usage(snap: UsageSnapshot, usage: Any) -> None:
    if not isinstance(usage, dict):
        return
    vendor_input = _as_int(usage.get("input_tokens"))  # total
    cached = 0
    details_in = usage.get("input_tokens_details")
    if isinstance(details_in, dict):
        cached = _as_int(details_in.get("cached_tokens"))
    uncached = max(0, vendor_input - cached)
    snap.cached_input_tokens = max(snap.cached_input_tokens, cached)
    snap.uncached_input_tokens = max(snap.uncached_input_tokens, uncached)
    snap.total_input_tokens = max(snap.total_input_tokens, vendor_input)
    # OpenAI does not separately report cache-creation (the provider
    # manages caching implicitly); leave ``cache_creation_input_tokens``
    # at 0.
    output_tokens = _as_int(usage.get("output_tokens"))
    reasoning = 0
    details_out = usage.get("output_tokens_details")
    if isinstance(details_out, dict):
        reasoning = _as_int(details_out.get("reasoning_tokens"))
    snap.reasoning_output_tokens = max(snap.reasoning_output_tokens, reasoning)
    non_reasoning = max(0, output_tokens - reasoning)
    snap.non_reasoning_output_tokens = max(snap.non_reasoning_output_tokens, non_reasoning)
    snap.total_tokens = max(snap.total_tokens, _as_int(usage.get("total_tokens")))


# ---------------------------------------------------------------------------
# Byte-counting helpers
# ---------------------------------------------------------------------------


def _utf8_len(x: Any) -> int:
    if isinstance(x, str):
        return len(x.encode("utf-8"))
    return 0


def _string_leaf_bytes(payload: Any) -> int:
    """Sum UTF-8 lengths of every string leaf in ``payload``."""

    if isinstance(payload, str):
        return len(payload.encode("utf-8"))
    total = 0
    stack: list[Any] = [payload]
    while stack:
        cur = stack.pop()
        if isinstance(cur, str):
            total += len(cur.encode("utf-8"))
        elif isinstance(cur, dict):
            stack.extend(cur.values())
        elif isinstance(cur, list):
            stack.extend(cur)
    return total


def _bump_tool_name(br: ContentBreakdown, name: Any) -> None:
    if isinstance(name, str) and name:
        br.tool_names[name] = br.tool_names.get(name, 0) + 1


def _as_int(value: Any) -> int:
    if isinstance(value, bool):
        return int(value)
    if isinstance(value, (int, float)):
        return int(value)
    return 0
