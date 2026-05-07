#!/usr/bin/env python3
"""Print a semantic per-event trace from a proxy ``--dump-dir`` directory.

Reads the ``NNNN.req`` / ``NNNN.resp`` / ``NNNN.meta`` files produced by
``Gateway(dump_dir=...)`` and prints the ORDERED items in each request
and response: system prompts, tool schema lists, user messages, tool
calls (with names), tool results (with sizes), assistant text, and
reasoning blocks.

Use this to decide where the harness-bootstrap ends and the model-driven
agent loop begins — e.g. whether to cut at the first request or at the
first tool call.

Usage::

    uv run python scripts/proxy_trace.py /tmp/bnf-trace/<session_id>
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any, Iterable


def _fmt(n: int) -> str:
    if n < 1024:
        return f"{n}B"
    if n < 1024 * 1024:
        return f"{n / 1024:.1f}KB"
    return f"{n / 1024 / 1024:.2f}MB"


def _utf8_len(x: Any) -> int:
    return len(x.encode("utf-8")) if isinstance(x, str) else 0


def _leaf_bytes(payload: Any) -> int:
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


# ---------------------------------------------------------------------------
# Anthropic request / response
# ---------------------------------------------------------------------------


def trace_anthropic_request(body: bytes) -> list[str]:
    try:
        obj = json.loads(body)
    except Exception:
        return [f"[not JSON, {len(body)}B]"]
    out: list[str] = []
    sys_ = obj.get("system")
    if sys_:
        out.append(f"  system={_fmt(_leaf_bytes(sys_))}")
    tools = obj.get("tools")
    if isinstance(tools, list):
        names = [t.get("name", "?") for t in tools if isinstance(t, dict)]
        out.append(f"  tools[{len(names)}]={names}  ({_fmt(_leaf_bytes(tools))})")
    for i, msg in enumerate(obj.get("messages") or []):
        role = msg.get("role")
        content = msg.get("content")
        if isinstance(content, str):
            preview = _preview(content)
            out.append(f"  msg[{i}] {role}: text {_fmt(_utf8_len(content))} {preview}")
            continue
        if not isinstance(content, list):
            continue
        for j, block in enumerate(content):
            if not isinstance(block, dict):
                continue
            btype = block.get("type")
            if btype == "text":
                out.append(
                    f"  msg[{i}.{j}] {role} text: {_fmt(_utf8_len(block.get('text')))}"
                    f" {_preview(block.get('text'))}"
                )
            elif btype == "tool_use":
                name = block.get("name")
                inp = block.get("input") or {}
                args = json.dumps(inp, ensure_ascii=False)[:100]
                out.append(f"  msg[{i}.{j}] {role} tool_use → {name}  args={args}")
            elif btype == "tool_result":
                n = _leaf_bytes(block.get("content"))
                tool_use_id = block.get("tool_use_id", "?")
                out.append(f"  msg[{i}.{j}] {role} tool_result (id={tool_use_id[-8:]}): {_fmt(n)}")
            elif btype == "thinking":
                out.append(
                    f"  msg[{i}.{j}] {role} thinking: {_fmt(_utf8_len(block.get('thinking')))}"
                )
    return out


def trace_anthropic_response(body: bytes) -> list[str]:
    events = list(_parse_sse(body))
    blocks: dict[int, dict[str, Any]] = {}
    out: list[str] = []
    for ev in events:
        t = ev.get("type")
        if t == "content_block_start":
            idx = ev.get("index")
            block = ev.get("content_block") or {}
            btype = block.get("type")
            blocks[idx] = {"type": btype, "bytes": 0, "partial": ""}
            if btype == "tool_use":
                blocks[idx]["name"] = block.get("name")
                blocks[idx]["partial_json"] = ""
        elif t == "content_block_delta":
            idx = ev.get("index")
            delta = ev.get("delta") or {}
            slot = blocks.get(idx)
            if slot is None:
                continue
            if slot["type"] == "text" and delta.get("type") == "text_delta":
                txt = delta.get("text") or ""
                slot["bytes"] += _utf8_len(txt)
                slot["partial"] += txt
            elif slot["type"] == "tool_use" and delta.get("type") == "input_json_delta":
                pj = delta.get("partial_json") or ""
                slot["bytes"] += _utf8_len(pj)
                slot["partial_json"] += pj
            elif slot["type"] == "thinking" and delta.get("type") == "thinking_delta":
                txt = delta.get("thinking") or ""
                slot["bytes"] += _utf8_len(txt)
                slot["partial"] += txt
        elif t == "content_block_stop":
            idx = ev.get("index")
            slot = blocks.get(idx)
            if slot is None:
                continue
            btype = slot["type"]
            if btype == "text":
                out.append(
                    f"  out[{idx}] assistant text: {_fmt(slot['bytes'])}"
                    f" {_preview(slot['partial'])}"
                )
            elif btype == "tool_use":
                pj = slot.get("partial_json") or ""
                args_summary = pj[:100].replace("\n", " ") if pj else ""
                out.append(
                    f"  out[{idx}] assistant tool_use → {slot.get('name')}  args={args_summary}"
                )
            elif btype == "thinking":
                out.append(f"  out[{idx}] assistant thinking: {_fmt(slot['bytes'])}")
    return out


# ---------------------------------------------------------------------------
# OpenAI Responses request / response
# ---------------------------------------------------------------------------


def trace_openai_request(body: bytes) -> list[str]:
    try:
        obj = json.loads(body)
    except Exception:
        return [f"[not JSON, {len(body)}B]"]
    out: list[str] = []
    instr = obj.get("instructions")
    if isinstance(instr, str):
        out.append(f"  instructions={_fmt(_utf8_len(instr))}")
    tools = obj.get("tools")
    if isinstance(tools, list):
        names = [t.get("name", t.get("type", "?")) for t in tools if isinstance(t, dict)]
        out.append(
            f"  tools[{len(names)}]={names[:10]}{'...' if len(names) > 10 else ''}"
            f"  ({_fmt(_leaf_bytes(tools))})"
        )
    input_items = obj.get("input")
    if isinstance(input_items, str):
        out.append(f"  input str: {_fmt(_utf8_len(input_items))} {_preview(input_items)}")
    elif isinstance(input_items, list):
        for i, item in enumerate(input_items):
            if not isinstance(item, dict):
                continue
            itype = item.get("type")
            role = item.get("role")
            if itype == "message" or (itype is None and role):
                content = item.get("content")
                n = _leaf_bytes(content)
                preview = ""
                if isinstance(content, list):
                    for part in content:
                        if isinstance(part, dict) and part.get("type") in (
                            "input_text",
                            "text",
                            "output_text",
                        ):
                            preview = _preview(part.get("text"))
                            break
                out.append(f"  in[{i}] {role or 'message'}: {_fmt(n)} {preview}")
            elif itype == "function_call":
                name = item.get("name")
                args = (item.get("arguments") or "")[:100].replace("\n", " ")
                out.append(f"  in[{i}] function_call → {name}  args={args}")
            elif itype == "function_call_output":
                out.append(
                    f"  in[{i}] function_call_output (call={item.get('call_id', '?')[-8:]}): "
                    f"{_fmt(_leaf_bytes(item.get('output')))}"
                )
            elif itype in ("reasoning", "reasoning_text", "reasoning_summary"):
                out.append(f"  in[{i}] reasoning echo: {_fmt(_leaf_bytes(item))}")
    return out


def trace_openai_response(body: bytes) -> list[str]:
    events = list(_parse_sse(body))
    items: dict[str, dict[str, Any]] = {}
    order: list[str] = []
    out: list[str] = []
    for ev in events:
        t = ev.get("type")
        if t == "response.output_item.added":
            item = ev.get("item") or {}
            iid = item.get("id") or f"idx{ev.get('output_index')}"
            slot = {"type": item.get("type"), "bytes": 0, "text": "", "args": ""}
            if slot["type"] == "function_call":
                slot["name"] = item.get("name")
            items[iid] = slot
            order.append(iid)
        elif t == "response.output_text.delta":
            iid = ev.get("item_id") or (order[-1] if order else None)
            if iid and iid in items:
                d = ev.get("delta") or ""
                items[iid]["bytes"] += _utf8_len(d)
                items[iid]["text"] += d
        elif t in ("response.reasoning_text.delta", "response.reasoning_summary_text.delta"):
            iid = ev.get("item_id") or (order[-1] if order else None)
            if iid and iid in items:
                d = ev.get("delta") or ""
                items[iid]["bytes"] += _utf8_len(d)
                items[iid]["text"] += d
        elif t == "response.function_call_arguments.delta":
            iid = ev.get("item_id") or (order[-1] if order else None)
            if iid and iid in items:
                d = ev.get("delta") or ""
                items[iid]["bytes"] += _utf8_len(d)
                items[iid]["args"] += d
    for iid in order:
        slot = items[iid]
        typ = slot["type"]
        if typ == "message":
            out.append(f"  out assistant text: {_fmt(slot['bytes'])} {_preview(slot['text'])}")
        elif typ == "function_call":
            args_summary = slot["args"][:100].replace("\n", " ")
            out.append(f"  out function_call → {slot.get('name')}  args={args_summary}")
        elif typ == "reasoning":
            out.append(f"  out reasoning: {_fmt(slot['bytes'])}")
        else:
            out.append(f"  out {typ}: {_fmt(slot['bytes'])}")
    return out


# ---------------------------------------------------------------------------
# SSE framing
# ---------------------------------------------------------------------------


def _parse_sse(body: bytes) -> Iterable[dict[str, Any]]:
    buf = body.replace(b"\r\n", b"\n").replace(b"\r", b"\n")
    for chunk in buf.split(b"\n\n"):
        if not chunk.strip():
            continue
        data_lines = []
        for line in chunk.split(b"\n"):
            if line.startswith(b"data:"):
                payload = line[5:]
                if payload.startswith(b" "):
                    payload = payload[1:]
                data_lines.append(payload.decode("utf-8", errors="replace"))
        if not data_lines:
            continue
        data = "\n".join(data_lines).strip()
        if not data or data == "[DONE]":
            continue
        try:
            obj = json.loads(data)
        except json.JSONDecodeError:
            continue
        if isinstance(obj, dict):
            yield obj


def _preview(text: Any, width: int = 72) -> str:
    if not isinstance(text, str):
        return ""
    single = " ".join(text.split())
    if len(single) <= width:
        return f"«{single}»"
    return f"«{single[:width]}…»"


# ---------------------------------------------------------------------------
# Driver
# ---------------------------------------------------------------------------


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("session_dir", type=Path, help="dump directory for one session")
    args = ap.parse_args()

    d: Path = args.session_dir
    if not d.is_dir():
        print(f"not a dir: {d}", file=sys.stderr)
        return 1

    indices = sorted(
        int(m.group(1))
        for m in (re.match(r"(\d+)\.meta$", p.name) for p in d.iterdir() if p.is_file())
        if m
    )

    # Sniff vendor from first meta.
    meta0 = (d / f"{indices[0]:04d}.meta").read_text(encoding="utf-8")
    is_anthropic = "/v1/messages" in meta0
    kind = "anthropic" if is_anthropic else "openai"
    print(f"# session={d.name} vendor={kind} events={len(indices)}")

    for idx in indices:
        meta = (d / f"{idx:04d}.meta").read_text(encoding="utf-8").strip()
        req = (d / f"{idx:04d}.req").read_bytes()
        resp = (d / f"{idx:04d}.resp").read_bytes()
        method_path = meta.splitlines()[0]
        print(
            f"\n=== event #{idx:02d}  {method_path}  req={_fmt(len(req))} resp={_fmt(len(resp))} ==="
        )
        print("REQUEST:")
        if is_anthropic:
            for line in trace_anthropic_request(req):
                print(line)
        else:
            for line in trace_openai_request(req):
                print(line)
        print("RESPONSE:")
        if is_anthropic:
            for line in trace_anthropic_response(resp):
                print(line)
        else:
            for line in trace_openai_response(resp):
                print(line)

    return 0


if __name__ == "__main__":
    sys.exit(main())
