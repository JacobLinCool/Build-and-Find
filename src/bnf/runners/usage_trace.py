"""Structured usage collection from persistent agent session logs.

The benchmark's efficiency numbers should come from the agent harness's
own persisted session/project logs rather than ad-hoc stdout parsing.
Stdout is used only to discover the session/thread identifier and a few
summary fields that are not present in the persisted logs.
"""

from __future__ import annotations

import dataclasses
import json
import os
from pathlib import Path
from typing import TYPE_CHECKING, Any, Iterable

from bnf.schema.run import UsageEvent, UsageMetrics, VendorUsage

if TYPE_CHECKING:
    from bnf.runners.base import AgentInvocation


_CLAUDE_NETWORK_TOOLS = {"WebFetch", "WebSearch"}
_CODEX_NETWORK_TOOLS = {"web.run", "web_fetch", "web_search"}


@dataclasses.dataclass
class UsageCollection:
    usage: UsageMetrics
    events: list[UsageEvent] = dataclasses.field(default_factory=list)


def write_usage_events(path: Path, events: list[UsageEvent]) -> None:
    with path.open("w", encoding="utf-8") as f:
        for event in events:
            f.write(event.model_dump_json())
            f.write("\n")


def collect_codex_usage(
    inv: "AgentInvocation",
    stdout: str,
    stderr: str,
    elapsed: float,
    *,
    codex_home: Path | None = None,
) -> UsageCollection:
    del stderr
    thread_id = _parse_codex_thread_id(stdout)
    if thread_id is None:
        raise RuntimeError("codex usage collection failed: missing thread.started event")

    trace_path = _find_codex_trace_path(thread_id, codex_home=codex_home)
    events: list[UsageEvent] = []
    prompt_bytes = len(inv.prompt.encode("utf-8"))
    retrieval_payload_bytes = 0
    tool_input_bytes = 0
    assistant_message_bytes = 0
    final_answer_bytes = 0
    tool_calls = 0
    network_requests = 0
    seen_calls: set[str] = set()
    seen_outputs: set[str] = set()
    vendor_usage: VendorUsage | None = None

    for obj in _iter_jsonl_file(trace_path):
        otype = obj.get("type")
        if otype == "response_item":
            payload = obj.get("payload")
            if not isinstance(payload, dict):
                continue
            ptype = payload.get("type")
            if ptype in ("function_call", "custom_tool_call"):
                call_id = payload.get("call_id")
                if not isinstance(call_id, str) or call_id in seen_calls:
                    continue
                seen_calls.add(call_id)
                name = payload.get("name")
                input_payload = payload.get("arguments")
                if ptype == "custom_tool_call":
                    input_payload = payload.get("input")
                text_bytes = _string_leaf_bytes(input_payload)
                is_network = _is_codex_network_tool(name)
                tool_calls += 1
                if is_network:
                    network_requests += 1
                tool_input_bytes += text_bytes
                events.append(
                    UsageEvent(
                        index=len(events),
                        source="session_log",
                        direction="model_to_harness",
                        kind="tool_call",
                        name=name if isinstance(name, str) else None,
                        event_id=call_id,
                        text_bytes=text_bytes,
                        is_network=is_network,
                    )
                )
            elif ptype in ("function_call_output", "custom_tool_call_output"):
                call_id = payload.get("call_id")
                if not isinstance(call_id, str) or call_id in seen_outputs:
                    continue
                seen_outputs.add(call_id)
                text_bytes = _string_leaf_bytes(payload.get("output"))
                retrieval_payload_bytes += text_bytes
                events.append(
                    UsageEvent(
                        index=len(events),
                        source="session_log",
                        direction="harness_to_model",
                        kind="tool_result",
                        event_id=call_id,
                        text_bytes=text_bytes,
                    )
                )
            elif ptype == "message" and payload.get("role") == "assistant":
                text_bytes = 0
                for block in payload.get("content") or []:
                    if isinstance(block, dict) and block.get("type") == "output_text":
                        text_bytes += _string_leaf_bytes(block.get("text"))
                if text_bytes == 0:
                    continue
                assistant_message_bytes += text_bytes
                kind = "assistant_text"
                if payload.get("phase") == "final_answer":
                    final_answer_bytes += text_bytes
                    kind = "final_answer"
                events.append(
                    UsageEvent(
                        index=len(events),
                        source="session_log",
                        direction="model_to_harness",
                        kind=kind,  # type: ignore[arg-type]
                        text_bytes=text_bytes,
                    )
                )
        elif otype == "event_msg":
            payload = obj.get("payload")
            if not isinstance(payload, dict) or payload.get("type") != "token_count":
                continue
            candidate = _codex_vendor_usage_from_token_count(payload)
            if candidate is None:
                continue
            if vendor_usage is None or candidate.total_tokens >= vendor_usage.total_tokens:
                vendor_usage = candidate

    if vendor_usage is not None:
        events.append(
            UsageEvent(
                index=len(events),
                source="session_log",
                direction="metadata",
                kind="vendor_usage",
                text_bytes=0,
            )
        )

    usage = UsageMetrics(
        wall_clock_seconds=elapsed,
        usage_source="session_log",
        session_id=thread_id,
        trace_path=str(trace_path),
        prompt_bytes=prompt_bytes,
        retrieval_payload_bytes=retrieval_payload_bytes,
        tool_input_bytes=tool_input_bytes,
        assistant_message_bytes=assistant_message_bytes,
        final_answer_bytes=final_answer_bytes,
        bytes_to_model=prompt_bytes + retrieval_payload_bytes,
        bytes_from_model=tool_input_bytes + assistant_message_bytes,
        tool_calls=tool_calls,
        network_requests=network_requests,
        vendor_usage=vendor_usage,
    )
    return UsageCollection(usage=usage, events=events)


def collect_claude_usage(
    inv: "AgentInvocation",
    stdout: str,
    stderr: str,
    elapsed: float,
    *,
    claude_home: Path | None = None,
) -> UsageCollection:
    del stderr
    session_id = _parse_claude_session_id(stdout)
    if session_id is None:
        raise RuntimeError("claude usage collection failed: missing session_id in stream")

    trace_path = _find_claude_trace_path(session_id, claude_home=claude_home)
    trace_files = [trace_path]
    subagents_dir = trace_path.parent / "subagents"
    if subagents_dir.exists():
        trace_files.extend(sorted(subagents_dir.glob("*.jsonl")))

    result_usage, final_answer_bytes, total_cost_usd = _parse_claude_stream_result(stdout)
    events: list[UsageEvent] = []
    prompt_bytes = len(inv.prompt.encode("utf-8"))
    retrieval_payload_bytes = 0
    tool_input_bytes = 0
    assistant_message_bytes = 0
    tool_calls = 0
    network_requests = 0
    seen_tool_uses: set[str] = set()
    seen_tool_results: set[str] = set()
    request_usage: dict[str, VendorUsage] = {}

    for file_path in trace_files:
        for obj in _iter_jsonl_file(file_path):
            if obj.get("sessionId") not in (None, session_id):
                continue
            otype = obj.get("type")
            message = obj.get("message")
            if otype == "assistant" and isinstance(message, dict):
                req_id = obj.get("requestId")
                usage = message.get("usage")
                if isinstance(req_id, str) and isinstance(usage, dict):
                    _merge_claude_request_usage(request_usage, req_id, usage)
                for block in message.get("content") or []:
                    if not isinstance(block, dict):
                        continue
                    btype = block.get("type")
                    if btype == "text":
                        text_bytes = _string_leaf_bytes(block.get("text"))
                        if text_bytes == 0:
                            continue
                        assistant_message_bytes += text_bytes
                        events.append(
                            UsageEvent(
                                index=len(events),
                                source="project_log",
                                direction="model_to_harness",
                                kind="assistant_text",
                                text_bytes=text_bytes,
                            )
                        )
                    elif btype == "tool_use":
                        tool_use_id = block.get("id")
                        if not isinstance(tool_use_id, str) or tool_use_id in seen_tool_uses:
                            continue
                        seen_tool_uses.add(tool_use_id)
                        name = block.get("name")
                        text_bytes = _string_leaf_bytes(block.get("input"))
                        is_network = isinstance(name, str) and name in _CLAUDE_NETWORK_TOOLS
                        tool_calls += 1
                        if is_network:
                            network_requests += 1
                        tool_input_bytes += text_bytes
                        events.append(
                            UsageEvent(
                                index=len(events),
                                source="project_log",
                                direction="model_to_harness",
                                kind="tool_call",
                                name=name if isinstance(name, str) else None,
                                event_id=tool_use_id,
                                text_bytes=text_bytes,
                                is_network=is_network,
                            )
                        )
            elif otype == "user" and isinstance(message, dict):
                content = message.get("content") or []
                if isinstance(content, str):
                    content = [content]
                for block in content:
                    if not isinstance(block, dict) or block.get("type") != "tool_result":
                        continue
                    tool_use_id = block.get("tool_use_id")
                    if not isinstance(tool_use_id, str) or tool_use_id in seen_tool_results:
                        continue
                    seen_tool_results.add(tool_use_id)
                    text_bytes = _string_leaf_bytes(block.get("content"))
                    retrieval_payload_bytes += text_bytes
                    events.append(
                        UsageEvent(
                            index=len(events),
                            source="project_log",
                            direction="harness_to_model",
                            kind="tool_result",
                            event_id=tool_use_id,
                            text_bytes=text_bytes,
                        )
                    )

    vendor_usage = _sum_vendor_usage(request_usage.values())
    if result_usage is not None:
        vendor_usage = _merge_result_usage(vendor_usage, result_usage)
    if vendor_usage.total_tokens > 0 or vendor_usage.cost_usd is not None:
        events.append(
            UsageEvent(
                index=len(events),
                source="project_log",
                direction="metadata",
                kind="vendor_usage",
                text_bytes=0,
            )
        )

    usage = UsageMetrics(
        wall_clock_seconds=elapsed,
        usage_source="project_log",
        session_id=session_id,
        trace_path=str(trace_path),
        prompt_bytes=prompt_bytes,
        retrieval_payload_bytes=retrieval_payload_bytes,
        tool_input_bytes=tool_input_bytes,
        assistant_message_bytes=assistant_message_bytes,
        final_answer_bytes=final_answer_bytes,
        bytes_to_model=prompt_bytes + retrieval_payload_bytes,
        bytes_from_model=tool_input_bytes + assistant_message_bytes,
        tool_calls=tool_calls,
        network_requests=network_requests,
        vendor_usage=vendor_usage,
    )
    return UsageCollection(usage=usage, events=events)


def _parse_codex_thread_id(stdout: str) -> str | None:
    for obj in _iter_json_lines(stdout):
        if obj.get("type") == "thread.started":
            thread_id = obj.get("thread_id")
            if isinstance(thread_id, str):
                return thread_id
    return None


def _find_codex_trace_path(thread_id: str, *, codex_home: Path | None = None) -> Path:
    if codex_home is None:
        codex_home = Path(os.environ.get("CODEX_HOME") or (Path.home() / ".codex"))
    matches = sorted((codex_home / "sessions").rglob(f"*{thread_id}.jsonl"))
    if len(matches) != 1:
        raise RuntimeError(
            f"codex usage collection failed: expected 1 session trace for {thread_id}, "
            f"found {len(matches)}"
        )
    return matches[0]


def _codex_vendor_usage_from_token_count(payload: dict[str, Any]) -> VendorUsage | None:
    info = payload.get("info")
    if not isinstance(info, dict):
        return None
    total = info.get("total_token_usage")
    if not isinstance(total, dict):
        return None
    return VendorUsage(
        source="session_log",
        input_tokens=_as_int(total.get("input_tokens")),
        cache_read_input_tokens=_as_int(total.get("cached_input_tokens")),
        output_tokens=_as_int(total.get("output_tokens")),
        reasoning_output_tokens=_as_int(total.get("reasoning_output_tokens")),
        total_tokens=_as_int(total.get("total_tokens")),
    )


def _parse_claude_session_id(stdout: str) -> str | None:
    for obj in _iter_json_lines(stdout):
        session_id = obj.get("session_id")
        if isinstance(session_id, str):
            return session_id
        if obj.get("type") == "system":
            session_id = obj.get("session_id")
            if isinstance(session_id, str):
                return session_id
    return None


def _find_claude_trace_path(
    session_id: str, *, claude_home: Path | None = None
) -> Path:
    if claude_home is None:
        claude_home = Path(os.environ.get("CLAUDE_CONFIG_DIR") or (Path.home() / ".claude"))
    matches = sorted((claude_home / "projects").rglob(f"{session_id}.jsonl"))
    if len(matches) != 1:
        raise RuntimeError(
            f"claude usage collection failed: expected 1 project trace for {session_id}, "
            f"found {len(matches)}"
        )
    return matches[0]


def _parse_claude_stream_result(stdout: str) -> tuple[VendorUsage | None, int, float | None]:
    final_answer_bytes = 0
    total_cost_usd: float | None = None
    result_usage: VendorUsage | None = None
    for obj in _iter_json_lines(stdout):
        if obj.get("type") != "result":
            continue
        final_answer_bytes = _string_leaf_bytes(obj.get("result"))
        usage = obj.get("usage")
        if isinstance(usage, dict):
            result_usage = VendorUsage(
                source="stream_result",
                input_tokens=_as_int(usage.get("input_tokens")),
                cache_creation_input_tokens=_as_int(usage.get("cache_creation_input_tokens")),
                cache_read_input_tokens=_as_int(usage.get("cache_read_input_tokens")),
                output_tokens=_as_int(usage.get("output_tokens")),
            )
            result_usage.total_tokens = (
                result_usage.input_tokens
                + result_usage.cache_creation_input_tokens
                + result_usage.cache_read_input_tokens
                + result_usage.output_tokens
            )
        cost = obj.get("total_cost_usd")
        if isinstance(cost, (int, float)):
            total_cost_usd = float(cost)
    if result_usage is not None:
        result_usage.cost_usd = total_cost_usd
    return result_usage, final_answer_bytes, total_cost_usd


def _merge_claude_request_usage(
    request_usage: dict[str, VendorUsage],
    request_id: str,
    usage: dict[str, Any],
) -> None:
    current = request_usage.get(request_id) or VendorUsage(source="project_log")
    current.input_tokens = max(current.input_tokens, _as_int(usage.get("input_tokens")))
    current.cache_creation_input_tokens = max(
        current.cache_creation_input_tokens,
        _as_int(usage.get("cache_creation_input_tokens")),
    )
    current.cache_read_input_tokens = max(
        current.cache_read_input_tokens,
        _as_int(usage.get("cache_read_input_tokens")),
    )
    current.output_tokens = max(current.output_tokens, _as_int(usage.get("output_tokens")))
    current.total_tokens = (
        current.input_tokens
        + current.cache_creation_input_tokens
        + current.cache_read_input_tokens
        + current.output_tokens
    )
    request_usage[request_id] = current


def _sum_vendor_usage(records: Iterable[VendorUsage]) -> VendorUsage:
    total = VendorUsage(source="project_log")
    for record in records:
        total.input_tokens += record.input_tokens
        total.cache_creation_input_tokens += record.cache_creation_input_tokens
        total.cache_read_input_tokens += record.cache_read_input_tokens
        total.output_tokens += record.output_tokens
        total.reasoning_output_tokens += record.reasoning_output_tokens
        total.total_tokens += record.total_tokens
        if record.cost_usd is not None:
            total.cost_usd = (total.cost_usd or 0.0) + record.cost_usd
    return total


def _merge_result_usage(base: VendorUsage, stream_result: VendorUsage) -> VendorUsage:
    merged = base.model_copy(deep=True)
    merged.input_tokens = max(merged.input_tokens, stream_result.input_tokens)
    merged.cache_creation_input_tokens = max(
        merged.cache_creation_input_tokens, stream_result.cache_creation_input_tokens
    )
    merged.cache_read_input_tokens = max(
        merged.cache_read_input_tokens, stream_result.cache_read_input_tokens
    )
    merged.output_tokens = max(merged.output_tokens, stream_result.output_tokens)
    merged.total_tokens = max(merged.total_tokens, stream_result.total_tokens)
    if stream_result.cost_usd is not None:
        merged.cost_usd = stream_result.cost_usd
    return merged


def _iter_json_lines(text: str) -> Iterable[dict[str, Any]]:
    for line in text.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(obj, dict):
            yield obj


def _iter_jsonl_file(path: Path) -> Iterable[dict[str, Any]]:
    with path.open("r", encoding="utf-8") as f:
        for raw in f:
            raw = raw.strip()
            if not raw.startswith("{"):
                continue
            try:
                obj = json.loads(raw)
            except json.JSONDecodeError:
                continue
            if isinstance(obj, dict):
                yield obj


def _string_leaf_bytes(payload: Any) -> int:
    if isinstance(payload, str):
        return len(payload.encode("utf-8"))
    total = 0
    stack = [payload]
    while stack:
        cur = stack.pop()
        if isinstance(cur, str):
            total += len(cur.encode("utf-8"))
        elif isinstance(cur, dict):
            stack.extend(cur.values())
        elif isinstance(cur, list):
            stack.extend(cur)
    return total


def _as_int(value: Any) -> int:
    if isinstance(value, bool):
        return int(value)
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        return int(value)
    return 0


def _is_codex_network_tool(name: Any) -> bool:
    if not isinstance(name, str):
        return False
    lowered = name.lower()
    return lowered in _CODEX_NETWORK_TOOLS or lowered.startswith("web_")
