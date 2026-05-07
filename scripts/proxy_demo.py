#!/usr/bin/env python3
"""Smoke test: run ``claude`` and/or ``codex`` through the Phase 1 proxy.

Purpose
-------
Verify end-to-end that:

1. ``Gateway.create_session`` yields a usable base URL.
2. The upstream agent CLI, pointed at that URL via the vendor env var, can
   authenticate and complete a trivial request.
3. Per-session stats (requests / body bytes / vendor tokens / events) are
   populated with sensible values.

Usage
-----

    uv run python scripts/proxy_demo.py --claude
    uv run python scripts/proxy_demo.py --codex
    uv run python scripts/proxy_demo.py --both

The script assumes the user is already logged in to each CLI (OAuth token
cached, ``ANTHROPIC_API_KEY`` / ``OPENAI_API_KEY`` set, or equivalent).
The proxy passes auth headers through verbatim.

Note on base URLs
-----------------

* Anthropic: ``ANTHROPIC_BASE_URL`` is the API root *without* ``/v1``.
  Claude Code appends ``/v1/messages``.
* OpenAI:   ``OPENAI_BASE_URL`` conventionally *includes* ``/v1``. Codex
  appends ``/responses``.

The demo sets each env var accordingly.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

from bnf.proxy import EventSlice, Gateway, SessionStats

DEFAULT_PROMPT = "Reply with exactly the single word OK and nothing else."
EXPLAIN_REPO_PROMPT = (
    "Explore this repository and explain what it does in 4-6 sentences. "
    "Read README.md, pyproject.toml, and at least three source files under src/ "
    "to ground your answer. End with a bullet list of the three files you "
    "found most load-bearing for understanding the project."
)


def _fmt_bytes(n: int) -> str:
    if n < 1024:
        return f"{n}B"
    if n < 1024 * 1024:
        return f"{n / 1024:.1f}KB"
    return f"{n / 1024 / 1024:.2f}MB"


def _print_stats(label: str, stats: SessionStats) -> None:
    print(f"\n[{label}] === session stats ===")
    print(f"  id                    = {stats.id}")
    print(f"  upstream              = {stats.upstream}")
    print(f"  requests              = {stats.requests}")
    print(f"  wall_clock_seconds    = {stats.wall_clock_seconds:.2f}")
    # Session-level timing totals (summed across events).
    total_wall = sum(e.wall_clock_seconds for e in stats.events)
    total_ttft = sum((e.ttft_seconds or 0) for e in stats.events)
    total_gen = sum((e.generation_seconds or 0) for e in stats.events)
    total_gap = sum(e.harness_gap_seconds for e in stats.events)
    out_total = stats.vendor.non_reasoning_output_tokens + stats.vendor.reasoning_output_tokens
    tok_s = (out_total / total_gen) if total_gen > 0 else None
    print(
        f"  timing totals         wall={total_wall:.2f}s "
        f"ttft={total_ttft:.2f}s gen={total_gen:.2f}s harness_gap={total_gap:.2f}s "
        + (f"tok/s={tok_s:.1f}" if tok_s is not None else "tok/s=—")
    )
    print(
        f"  raw HTTP traffic      = "
        f"req {_fmt_bytes(stats.raw_request_bytes)} / "
        f"resp {_fmt_bytes(stats.raw_response_bytes)}"
    )
    v = stats.vendor
    print(
        f"  input tokens          = total={v.total_input_tokens} "
        f"cached={v.cached_input_tokens} "
        f"uncached={v.uncached_input_tokens}"
        + (
            f" (cache_creation={v.cache_creation_input_tokens})"
            if v.cache_creation_input_tokens
            else ""
        )
    )
    print(
        f"  output tokens         = non_reasoning={v.non_reasoning_output_tokens} "
        f"reasoning={v.reasoning_output_tokens}"
    )
    print(f"  vendor total tokens   = {v.total_tokens}")
    ci = stats.content_to_model
    cn = stats.novel_content_to_model
    co = stats.content_from_model
    print("  content → model       wire | novel (deduped)")
    print(
        f"    user_text           {_fmt_bytes(ci.user_text_bytes):>10} | {_fmt_bytes(cn.user_text_bytes):>10}"
    )
    print(
        f"    tool_result         {_fmt_bytes(ci.tool_result_bytes):>10} | {_fmt_bytes(cn.tool_result_bytes):>10}"
        f"    count wire={ci.tool_result_count} novel={cn.tool_result_count}"
    )
    print(
        f"    assistant_echo      {_fmt_bytes(ci.assistant_text_bytes):>10} | {_fmt_bytes(cn.assistant_text_bytes):>10}"
    )
    print(
        f"    tool_call_echo      {_fmt_bytes(ci.tool_call_bytes):>10} | {_fmt_bytes(cn.tool_call_bytes):>10}"
        f"    count wire={ci.tool_call_count} novel={cn.tool_call_count}"
    )
    print(
        f"    reasoning_echo      {_fmt_bytes(ci.reasoning_bytes):>10} | {_fmt_bytes(cn.reasoning_bytes):>10}"
    )
    print(
        f"    system_prompt       {_fmt_bytes(ci.system_prompt_bytes):>10} | {_fmt_bytes(cn.system_prompt_bytes):>10}"
    )
    print(
        f"    tool_schema         {_fmt_bytes(ci.tool_schema_bytes):>10} | {_fmt_bytes(cn.tool_schema_bytes):>10}"
    )
    print(
        f"    conv_total ↑        {_fmt_bytes(ci.conversational_bytes):>10} | {_fmt_bytes(cn.conversational_bytes):>10}"
    )
    print(
        f"    overhead ↑          {_fmt_bytes(ci.overhead_bytes):>10} | {_fmt_bytes(cn.overhead_bytes):>10}"
    )
    print(
        "  content ← model       = "
        f"assistant={_fmt_bytes(co.assistant_text_bytes)} "
        f"tool_call={_fmt_bytes(co.tool_call_bytes)} (count={co.tool_call_count}) "
        f"reasoning={_fmt_bytes(co.reasoning_bytes)}"
    )

    def _range(lo: int, hi: int) -> str:
        if hi < lo:
            return "none"
        return f"#{lo}" if lo == hi else f"#{lo}..#{hi}"

    def _print_phase_split(tag: str, events_offset: int, slice_events: list) -> None:
        # compute indices-in-session for the label
        if not slice_events:
            return
        n = len(slice_events)
        # find loop_start within this slice
        first_tool_idx = next(
            (i for i, e in enumerate(slice_events) if e.content_from_model.tool_call_count > 0),
            n,
        )
        final_idx = n - 1
        bs_end = min(first_tool_idx, final_idx)
        abs_start = events_offset
        # Build three labeled slices by directly summarizing.
        # We use the same helpers the library uses for consistency.
        from bnf.proxy.gateway import _bootstrap_of, _driven_of, _final_of

        bs = _bootstrap_of(slice_events)
        dr = _driven_of(slice_events)
        fn = _final_of(slice_events)
        _print_slice(
            f"[{label}] {tag} bootstrap ({_range(abs_start, abs_start + bs_end - 1)} — pre-decision harness)",
            bs,
        )
        _print_slice(
            f"[{label}] {tag} driven    ({_range(abs_start + bs_end, abs_start + final_idx - 1)} — agent loop)",
            dr,
        )
        _print_slice(
            f"[{label}] {tag} final     ({_range(abs_start + final_idx, abs_start + final_idx)} — final output)",
            fn,
        )

    turns = stats.turns
    if len(turns) <= 1:
        # Single turn: print session-wide split (identical to turns[0]).
        _print_phase_split("", 0, stats.events)
    else:
        # Multi-turn: print per-turn splits.
        offset = 0
        for turn in turns:
            _print_phase_split(f"turn {turn.turn_index}", offset, turn.events)
            offset += len(turn.events)

    print(f"  events                = {len(stats.events)}")
    for evt in stats.events:
        usage_total = (evt.usage or {}).get("total_tokens")
        ci_e = evt.content_to_model
        co_e = evt.content_from_model
        ttft = evt.ttft_seconds
        gen = evt.generation_seconds
        timing = (
            f"ttft={ttft:.2f}s/gen={gen:.2f}s"
            if (ttft is not None and gen is not None)
            else f"wall={evt.wall_clock_seconds:.2f}s"
        )
        gap_str = f" gap={evt.harness_gap_seconds:.2f}s" if evt.harness_gap_seconds > 0 else ""
        print(
            f"    #{evt.index:02d} {evt.method:<6} {evt.path} "
            f"→ {evt.status} "
            f"raw={_fmt_bytes(evt.raw_request_bytes)}↑/{_fmt_bytes(evt.raw_response_bytes)}↓ "
            f"conv={_fmt_bytes(ci_e.conversational_bytes)}↑/{_fmt_bytes(co_e.conversational_bytes)}↓ "
            f"tool_calls={co_e.tool_call_count} "
            f"tok={usage_total or '-'} "
            f"{timing}{gap_str}"
            f"{(' err=' + evt.error) if evt.error else ''}"
        )


def _print_slice(title: str, s: EventSlice) -> None:
    if s.event_count == 0:
        return
    v = s.vendor
    ci = s.content_to_model
    cn = s.novel_content_to_model
    co = s.content_from_model
    print(f"\n  {title}  [{s.event_count} event(s)]")
    print(
        f"    input tokens   = total={v.total_input_tokens} "
        f"cached={v.cached_input_tokens} "
        f"uncached={v.uncached_input_tokens}"
    )
    print(
        f"    output tokens  = non_reasoning={v.non_reasoning_output_tokens} "
        f"reasoning={v.reasoning_output_tokens}"
    )
    tok_s = s.generation_tok_per_s
    print(
        f"    timing         wall={s.wall_clock_seconds:.2f}s "
        f"ttft={s.ttft_seconds:.2f}s "
        f"gen={s.generation_seconds:.2f}s "
        f"harness_gap={s.harness_gap_seconds:.2f}s "
        + (f"tok/s={tok_s:.1f}" if tok_s is not None else "tok/s=—")
    )
    print(
        f"    content ↑      wire:  conv={_fmt_bytes(ci.conversational_bytes)} "
        f"overhead={_fmt_bytes(ci.overhead_bytes)} "
        f"tool_calls={ci.tool_call_count} tool_results={ci.tool_result_count}"
    )
    print(
        f"                   novel: conv={_fmt_bytes(cn.conversational_bytes)} "
        f"overhead={_fmt_bytes(cn.overhead_bytes)} "
        f"tool_calls={cn.tool_call_count} tool_results={cn.tool_result_count}"
    )
    if co.tool_names:
        top = sorted(co.tool_names.items(), key=lambda kv: -kv[1])[:8]
        tools_summary = ", ".join(f"{n}×{c}" for n, c in top)
        print(f"    tools invoked  {tools_summary}")
    print(
        f"    content ↓      conv={_fmt_bytes(co.conversational_bytes)} "
        f"tool_calls={co.tool_call_count}"
    )


def _sanity_check(label: str, stats: SessionStats) -> bool:
    ok = True
    if stats.requests == 0:
        print(f"[{label}] FAIL: zero requests — CLI didn't use the proxy")
        ok = False
    if stats.raw_request_bytes == 0:
        print(f"[{label}] FAIL: raw_request_bytes == 0")
        ok = False
    if stats.raw_response_bytes == 0:
        print(f"[{label}] FAIL: raw_response_bytes == 0")
        ok = False
    if stats.vendor.total_tokens == 0:
        print(f"[{label}] WARN: vendor.total_tokens == 0 — parser found no usage")
    if stats.content_to_model.conversational_bytes == 0:
        print(f"[{label}] WARN: content_to_model has zero conversational bytes")
    return ok


def run_claude(gw: Gateway, model: str | None, timeout: int, prompt: str) -> bool:
    if shutil.which("claude") is None:
        print("[claude] SKIP: 'claude' not on PATH")
        return True
    session = gw.create_session(upstream="anthropic")
    env = os.environ.copy()
    env["ANTHROPIC_BASE_URL"] = session.base_url
    print(f"[claude] ANTHROPIC_BASE_URL={session.base_url}")
    argv = [
        "claude",
        "--print",
        "--output-format",
        "stream-json",
        "--verbose",
        "--dangerously-skip-permissions",
    ]
    if model:
        argv += ["--model", model]
    start = time.time()
    try:
        proc = subprocess.run(
            argv,
            input=prompt,
            capture_output=True,
            text=True,
            env=env,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        print(f"[claude] FAIL: timeout after {timeout}s")
        _print_stats("claude", gw.get_stats(session.id))
        return False
    elapsed = time.time() - start
    print(
        f"[claude] exit={proc.returncode} elapsed={elapsed:.1f}s "
        f"stdout_len={len(proc.stdout)} stderr_len={len(proc.stderr)}"
    )
    if proc.returncode != 0:
        print(f"[claude] stderr (first 500B):\n{proc.stderr[:500]}")
    stats = gw.get_stats(session.id)
    _print_stats("claude", stats)
    return _sanity_check("claude", stats) and proc.returncode == 0


def _detect_codex_auth_mode() -> str | None:
    """Return ``"chatgpt"``, ``"apikey"`` or ``None`` based on ~/.codex/auth.json."""

    path = Path.home() / ".codex" / "auth.json"
    if not path.exists():
        return None
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None
    mode = data.get("auth_mode")
    if mode in ("chatgpt", "apikey"):
        return mode
    if data.get("OPENAI_API_KEY"):
        return "apikey"
    if data.get("tokens"):
        return "chatgpt"
    return None


def run_codex(gw: Gateway, model: str | None, timeout: int, prompt: str) -> bool:
    if shutil.which("codex") is None:
        print("[codex] SKIP: 'codex' not on PATH")
        return True

    auth_mode = _detect_codex_auth_mode()
    print(f"[codex] detected auth_mode={auth_mode}")

    argv = [
        "codex",
        "exec",
        "--json",
        "--skip-git-repo-check",
        "--dangerously-bypass-approvals-and-sandbox",
    ]
    env = os.environ.copy()

    if auth_mode == "chatgpt":
        # Built-in ``openai`` provider is reserved. Define a custom provider
        # and select it via ``-c model_provider=``. ``requires_openai_auth``
        # preserves the ChatGPT-login credential flow; the session routes to
        # the ``chatgpt`` upstream which forwards to chatgpt.com/backend-api/codex.
        session = gw.create_session(upstream="chatgpt")
        argv += [
            "-c",
            'model_provider="openai-proxy"',
            "-c",
            'model_providers.openai-proxy.name="Proxy (ChatGPT)"',
            "-c",
            f'model_providers.openai-proxy.base_url="{session.base_url}"',
            "-c",
            'model_providers.openai-proxy.wire_api="responses"',
            "-c",
            "model_providers.openai-proxy.requires_openai_auth=true",
        ]
        print(f"[codex] model_provider=openai-proxy base_url={session.base_url}")
    else:
        # API-key mode: standard OPENAI_BASE_URL env var with /v1 suffix.
        session = gw.create_session(upstream="openai")
        env["OPENAI_BASE_URL"] = f"{session.base_url}/v1"
        print(f"[codex] OPENAI_BASE_URL={env['OPENAI_BASE_URL']}")

    if model:
        argv += ["--model", model]
    argv += [prompt]
    start = time.time()
    try:
        proc = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            env=env,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        print(f"[codex] FAIL: timeout after {timeout}s")
        _print_stats("codex", gw.get_stats(session.id))
        return False
    elapsed = time.time() - start
    print(
        f"[codex] exit={proc.returncode} elapsed={elapsed:.1f}s "
        f"stdout_len={len(proc.stdout)} stderr_len={len(proc.stderr)}"
    )
    if proc.returncode != 0:
        print(f"[codex] stderr (first 500B):\n{proc.stderr[:500]}")
    stats = gw.get_stats(session.id)
    _print_stats("codex", stats)
    return _sanity_check("codex", stats) and proc.returncode == 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--claude", action="store_true", help="run Claude Code")
    ap.add_argument("--codex", action="store_true", help="run Codex")
    ap.add_argument("--both", action="store_true", help="run both")
    ap.add_argument("--claude-model", default=None, help="e.g. claude-sonnet-4-6")
    ap.add_argument("--codex-model", default=None, help="e.g. gpt-5.4-mini")
    ap.add_argument("--timeout", type=int, default=300)
    ap.add_argument(
        "--dump-dir", default=None, help="dump raw request/response bodies under this directory"
    )
    ap.add_argument("--prompt", default=None, help="override the test prompt")
    ap.add_argument(
        "--explain-repo",
        action="store_true",
        help="use a tool-heavy 'explain this repo' prompt that exercises file reads and searches",
    )
    args = ap.parse_args()
    if not (args.claude or args.codex or args.both):
        ap.error("pick --claude, --codex, or --both")

    if args.prompt is not None:
        prompt = args.prompt
    elif args.explain_repo:
        prompt = EXPLAIN_REPO_PROMPT
    else:
        prompt = DEFAULT_PROMPT

    results: list[tuple[str, bool]] = []
    with Gateway.start(dump_dir=args.dump_dir) as gw:
        print(f"[gateway] listening on {gw.base_url}")
        print(f"[prompt] {prompt[:120]}{'...' if len(prompt) > 120 else ''}")
        if args.claude or args.both:
            results.append(("claude", run_claude(gw, args.claude_model, args.timeout, prompt)))
        if args.codex or args.both:
            results.append(("codex", run_codex(gw, args.codex_model, args.timeout, prompt)))

    print("\n=== summary ===")
    all_ok = True
    for label, ok in results:
        print(f"  {label:<8} {'OK' if ok else 'FAIL'}")
        all_ok &= ok
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
