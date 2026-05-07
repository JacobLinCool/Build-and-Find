"""Unit tests for the SSE / JSON usage + content parser."""

from __future__ import annotations

import json

from bnf.proxy.parser import (
    NovelTracker,
    StreamContentAnalyzer,
    StreamUsageParser,
    analyze_request_body,
    analyze_request_body_novel,
    analyze_response_body,
    parse_json_usage,
)


# ---------------------------------------------------------------------------
# UsageSnapshot (tokens)
# ---------------------------------------------------------------------------


def test_anthropic_stream_accumulates_message_start_and_delta() -> None:
    parser = StreamUsageParser("anthropic")
    events = [
        {
            "type": "message_start",
            "message": {
                "id": "msg_1",
                "usage": {
                    "input_tokens": 120,
                    "cache_creation_input_tokens": 0,
                    "cache_read_input_tokens": 45,
                    "output_tokens": 1,
                },
            },
        },
        {"type": "content_block_start", "index": 0},
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "hi"}},
        {
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 37},
        },
        {"type": "message_stop"},
    ]
    for ev in events:
        parser.feed(f"event: {ev['type']}\ndata: {json.dumps(ev)}\n\n".encode("utf-8"))
    parser.finalize()

    snap = parser.snapshot
    # Anthropic's raw input_tokens=120 is already "uncached-not-being-cached";
    # no cache_creation here, so uncached stays 120 and total adds cached.
    assert snap.uncached_input_tokens == 120
    assert snap.cached_input_tokens == 45
    assert snap.cache_creation_input_tokens == 0
    assert snap.total_input_tokens == 120 + 45
    assert snap.non_reasoning_output_tokens == 37
    assert snap.reasoning_output_tokens == 0
    assert snap.total_tokens == 120 + 45 + 37


def test_anthropic_stream_handles_split_chunks() -> None:
    payload = (
        b"event: message_start\n"
        b'data: {"type":"message_start","message":{"usage":{"input_tokens":10,"output_tokens":1}}}\n\n'
        b"event: message_delta\n"
        b'data: {"type":"message_delta","usage":{"output_tokens":42}}\n\n'
    )
    parser = StreamUsageParser("anthropic")
    for i in range(len(payload)):
        parser.feed(payload[i : i + 1])
    parser.finalize()
    assert parser.snapshot.uncached_input_tokens == 10
    assert parser.snapshot.total_input_tokens == 10
    assert parser.snapshot.non_reasoning_output_tokens == 42
    assert parser.snapshot.total_tokens == 10 + 42


def test_anthropic_stream_handles_crlf() -> None:
    parser = StreamUsageParser("anthropic")
    payload = (
        b"event: message_start\r\n"
        b'data: {"type":"message_start","message":{"usage":{"input_tokens":7,"output_tokens":0}}}\r\n\r\n'
    )
    parser.feed(payload)
    parser.finalize()
    assert parser.snapshot.uncached_input_tokens == 7
    assert parser.snapshot.total_input_tokens == 7


def test_openai_stream_picks_up_response_completed() -> None:
    parser = StreamUsageParser("openai")
    preamble_events = [
        {"type": "response.created", "response": {"id": "resp_1"}},
        {"type": "response.output_text.delta", "delta": "hi"},
    ]
    for ev in preamble_events:
        parser.feed(f"data: {json.dumps(ev)}\n\n".encode("utf-8"))
    final = {
        "type": "response.completed",
        "response": {
            "id": "resp_1",
            "usage": {
                "input_tokens": 200,
                "input_tokens_details": {"cached_tokens": 64},
                "output_tokens": 85,
                "output_tokens_details": {"reasoning_tokens": 30},
                "total_tokens": 285,
            },
        },
    }
    parser.feed(f"data: {json.dumps(final)}\n\n".encode("utf-8"))
    parser.feed(b"data: [DONE]\n\n")
    parser.finalize()

    snap = parser.snapshot
    # OpenAI's raw input_tokens=200 is the total (cached + uncached).
    assert snap.total_input_tokens == 200
    assert snap.cached_input_tokens == 64
    assert snap.uncached_input_tokens == 200 - 64
    assert snap.cache_creation_input_tokens == 0  # OpenAI doesn't report this
    assert snap.reasoning_output_tokens == 30
    assert snap.non_reasoning_output_tokens == 85 - 30
    assert snap.total_tokens == 285


def test_anthropic_json_body_tokens() -> None:
    body = json.dumps(
        {
            "id": "msg_1",
            "role": "assistant",
            "usage": {
                "input_tokens": 11,
                "cache_read_input_tokens": 2,
                "cache_creation_input_tokens": 3,
                "output_tokens": 4,
            },
        }
    ).encode("utf-8")
    snap = parse_json_usage(body, "anthropic")
    assert snap is not None
    # uncached = vendor input + cache_creation (both billed full).
    assert snap.uncached_input_tokens == 11 + 3
    assert snap.cached_input_tokens == 2
    assert snap.cache_creation_input_tokens == 3
    assert snap.total_input_tokens == 11 + 3 + 2
    assert snap.non_reasoning_output_tokens == 4
    assert snap.reasoning_output_tokens == 0
    assert snap.total_tokens == 11 + 3 + 2 + 4


def test_openai_json_body_with_details() -> None:
    body = json.dumps(
        {
            "id": "resp_1",
            "usage": {
                "input_tokens": 50,
                "input_tokens_details": {"cached_tokens": 10},
                "output_tokens": 20,
                "output_tokens_details": {"reasoning_tokens": 5},
                "total_tokens": 70,
            },
        }
    ).encode("utf-8")
    snap = parse_json_usage(body, "openai")
    assert snap is not None
    assert snap.total_input_tokens == 50
    assert snap.cached_input_tokens == 10
    assert snap.uncached_input_tokens == 40
    assert snap.cache_creation_input_tokens == 0
    assert snap.reasoning_output_tokens == 5
    assert snap.non_reasoning_output_tokens == 15
    assert snap.total_tokens == 70


def test_input_token_normalization_is_comparable_across_vendors() -> None:
    """The whole point of the renaming: same physical scenario → same numbers.

    Imagine both vendors report that a turn saw 1000 input tokens, 700 of
    which were cache hits. They report it with different schemas; our
    UsageSnapshot should produce matching fields."""

    anthropic_body = json.dumps(
        {
            "usage": {
                "input_tokens": 200,  # fresh, not-being-cached
                "cache_creation_input_tokens": 100,  # fresh, being cached
                "cache_read_input_tokens": 700,  # cached hit
                "output_tokens": 50,
            }
        }
    ).encode()
    openai_body = json.dumps(
        {
            "usage": {
                "input_tokens": 1000,  # total
                "input_tokens_details": {"cached_tokens": 700},
                "output_tokens": 50,
                "total_tokens": 1050,
            }
        }
    ).encode()

    a = parse_json_usage(anthropic_body, "anthropic")
    o = parse_json_usage(openai_body, "openai")
    assert a is not None and o is not None
    # Comparable fields match.
    assert a.total_input_tokens == o.total_input_tokens == 1000
    assert a.cached_input_tokens == o.cached_input_tokens == 700
    assert a.uncached_input_tokens == o.uncached_input_tokens == 300
    # Anthropic-only detail stays visible.
    assert a.cache_creation_input_tokens == 100
    assert o.cache_creation_input_tokens == 0


def test_json_body_without_usage_returns_none() -> None:
    assert parse_json_usage(b'{"id":"x"}', "anthropic") is None
    assert parse_json_usage(b"not json", "anthropic") is None
    assert parse_json_usage(b"", "openai") is None


# ---------------------------------------------------------------------------
# ContentBreakdown — Anthropic
# ---------------------------------------------------------------------------


def test_anthropic_request_body_buckets_bytes_correctly() -> None:
    system = "you are helpful"
    user_text = "hello there"
    tool_result_text = "file contents line one\nfile contents line two"
    assistant_prev = "thinking about it"
    tool_use_arg = "src/foo.py"
    tool_schema = {
        "name": "read_file",
        "description": "read a file by absolute path",
        "input_schema": {"type": "object", "properties": {"path": {"type": "string"}}},
    }

    body = json.dumps(
        {
            "model": "claude-sonnet-4-6",
            "max_tokens": 4096,
            "temperature": 1,
            "system": system,
            "messages": [
                {"role": "user", "content": user_text},
                {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": assistant_prev},
                        {
                            "type": "tool_use",
                            "id": "t1",
                            "name": "read_file",
                            "input": {"path": tool_use_arg},
                        },
                    ],
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "tool_result",
                            "tool_use_id": "t1",
                            "content": [{"type": "text", "text": tool_result_text}],
                        },
                    ],
                },
            ],
            "tools": [tool_schema],
        }
    ).encode("utf-8")

    br = analyze_request_body(body, "anthropic")
    assert br.system_prompt_bytes == len(system.encode("utf-8"))
    assert br.user_text_bytes == len(user_text.encode("utf-8"))
    assert br.tool_result_bytes == len(tool_result_text.encode("utf-8"))
    assert br.assistant_text_bytes == len(assistant_prev.encode("utf-8"))
    assert br.tool_call_bytes == len(tool_use_arg.encode("utf-8"))
    # tool_schema_bytes counts text leaves: name + description + "object" + "path" + "string"
    assert br.tool_schema_bytes > 0
    # Numbers (max_tokens, temperature) and model id must NOT leak into buckets.
    # model "claude-sonnet-4-6" is 19 chars but shouldn't appear anywhere.
    assert "claude-sonnet-4-6".encode("utf-8") not in json.dumps(br.to_dict()).encode("utf-8")


def test_anthropic_response_stream_buckets_delta_bytes() -> None:
    analyzer = StreamContentAnalyzer("anthropic")

    def send(obj: dict) -> None:
        analyzer.feed(f"event: {obj['type']}\ndata: {json.dumps(obj)}\n\n".encode("utf-8"))

    # Assistant text block
    send({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
    send(
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "hello"},
        }
    )
    send(
        {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": " world"},
        }
    )
    send({"type": "content_block_stop", "index": 0})

    # Tool_use block
    send(
        {
            "type": "content_block_start",
            "index": 1,
            "content_block": {"type": "tool_use", "id": "t1", "name": "read_file", "input": {}},
        }
    )
    send(
        {
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": '{"path":"src/foo.py"}'},
        }
    )
    send({"type": "content_block_stop", "index": 1})

    # Thinking block
    send(
        {
            "type": "content_block_start",
            "index": 2,
            "content_block": {"type": "thinking", "thinking": ""},
        }
    )
    send(
        {
            "type": "content_block_delta",
            "index": 2,
            "delta": {"type": "thinking_delta", "thinking": "think out loud"},
        }
    )
    send({"type": "content_block_stop", "index": 2})
    analyzer.finalize()

    br = analyzer.breakdown
    assert br.assistant_text_bytes == len("hello world".encode("utf-8"))
    assert br.tool_call_bytes == len('{"path":"src/foo.py"}'.encode("utf-8"))
    assert br.reasoning_bytes == len("think out loud".encode("utf-8"))


def test_anthropic_json_response_buckets() -> None:
    body = json.dumps(
        {
            "id": "msg_1",
            "content": [
                {"type": "text", "text": "hi there"},
                {"type": "tool_use", "id": "t1", "name": "read", "input": {"path": "a.py"}},
                {"type": "thinking", "thinking": "reasoning trace"},
            ],
            "usage": {"input_tokens": 1, "output_tokens": 2},
        }
    ).encode("utf-8")
    br = analyze_response_body(body, "anthropic")
    assert br.assistant_text_bytes == len("hi there".encode("utf-8"))
    assert br.tool_call_bytes == len("a.py".encode("utf-8"))
    assert br.reasoning_bytes == len("reasoning trace".encode("utf-8"))


# ---------------------------------------------------------------------------
# ContentBreakdown — OpenAI Responses
# ---------------------------------------------------------------------------


def test_openai_request_body_buckets_bytes_correctly() -> None:
    instructions = "you are a helpful coding agent"
    user_text = "fix the bug"
    assistant_prev = "I will start by reading the file"
    tool_result_text = "file contents line one\nfile contents line two"
    tool_call_args = '{"path": "src/foo.py"}'
    reasoning_text = "Thinking: check imports first"

    body = json.dumps(
        {
            "model": "gpt-5.4-mini",
            "reasoning": {"effort": "high"},
            "instructions": instructions,
            "input": [
                {
                    "role": "user",
                    "content": [{"type": "input_text", "text": user_text}],
                },
                {
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": assistant_prev}],
                },
                {"type": "function_call", "name": "read_file", "arguments": tool_call_args},
                {"type": "function_call_output", "call_id": "c1", "output": tool_result_text},
                {
                    "type": "reasoning",
                    "content": [{"type": "reasoning_text", "text": reasoning_text}],
                },
            ],
            "tools": [
                {"type": "function", "name": "read_file", "description": "reads a file"},
            ],
        }
    ).encode("utf-8")

    br = analyze_request_body(body, "openai")
    assert br.system_prompt_bytes == len(instructions.encode("utf-8"))
    assert br.user_text_bytes == len(user_text.encode("utf-8"))
    assert br.assistant_text_bytes == len(assistant_prev.encode("utf-8"))
    assert br.tool_call_bytes == len(tool_call_args.encode("utf-8"))
    assert br.tool_result_bytes == len(tool_result_text.encode("utf-8"))
    assert br.reasoning_bytes == len(reasoning_text.encode("utf-8"))
    assert br.tool_schema_bytes > 0


def test_openai_response_stream_buckets_delta_bytes() -> None:
    analyzer = StreamContentAnalyzer("openai")

    def send(obj: dict) -> None:
        analyzer.feed(f"data: {json.dumps(obj)}\n\n".encode("utf-8"))

    send({"type": "response.created", "response": {"id": "resp_1"}})
    send({"type": "response.output_text.delta", "delta": "hello"})
    send({"type": "response.output_text.delta", "delta": " world"})
    send({"type": "response.function_call_arguments.delta", "delta": '{"path":"x.py"}'})
    send({"type": "response.reasoning_text.delta", "delta": "step 1"})
    send({"type": "response.reasoning_summary_text.delta", "delta": " step 2"})
    analyzer.finalize()

    br = analyzer.breakdown
    assert br.assistant_text_bytes == len("hello world".encode("utf-8"))
    assert br.tool_call_bytes == len('{"path":"x.py"}'.encode("utf-8"))
    assert br.reasoning_bytes == len("step 1 step 2".encode("utf-8"))


def test_content_breakdown_conversational_sum() -> None:
    from bnf.proxy.parser import ContentBreakdown

    br = ContentBreakdown(
        user_text_bytes=10,
        tool_result_bytes=20,
        assistant_text_bytes=30,
        tool_call_bytes=40,
        reasoning_bytes=50,
        system_prompt_bytes=1000,
        tool_schema_bytes=2000,
    )
    assert br.conversational_bytes == 10 + 20 + 30 + 40 + 50
    assert br.overhead_bytes == 3000


def test_analyze_request_body_handles_non_json() -> None:
    br = analyze_request_body(b"not json at all", "anthropic")
    assert br.conversational_bytes == 0
    assert br.overhead_bytes == 0


# ---------------------------------------------------------------------------
# Tool-call / tool-result counts
# ---------------------------------------------------------------------------


def test_anthropic_tool_names_frequency_from_request_body() -> None:
    body = json.dumps(
        {
            "messages": [
                {
                    "role": "assistant",
                    "content": [
                        {"type": "tool_use", "id": "t1", "name": "Read", "input": {}},
                        {"type": "tool_use", "id": "t2", "name": "Read", "input": {}},
                        {"type": "tool_use", "id": "t3", "name": "Grep", "input": {}},
                    ],
                }
            ]
        }
    ).encode()
    br = analyze_request_body(body, "anthropic")
    assert br.tool_names == {"Read": 2, "Grep": 1}


def test_openai_tool_names_frequency_from_request_body() -> None:
    body = json.dumps(
        {
            "input": [
                {
                    "type": "function_call",
                    "name": "exec_command",
                    "call_id": "c1",
                    "arguments": "{}",
                },
                {
                    "type": "function_call",
                    "name": "exec_command",
                    "call_id": "c2",
                    "arguments": "{}",
                },
                {
                    "type": "function_call",
                    "name": "update_plan",
                    "call_id": "c3",
                    "arguments": "{}",
                },
            ]
        }
    ).encode()
    br = analyze_request_body(body, "openai")
    assert br.tool_names == {"exec_command": 2, "update_plan": 1}


def test_tool_names_merge_across_events_via_add() -> None:
    from bnf.proxy.parser import ContentBreakdown

    a = ContentBreakdown(tool_names={"Read": 1})
    b = ContentBreakdown(tool_names={"Read": 2, "Grep": 3})
    a.add(b)
    assert a.tool_names == {"Read": 3, "Grep": 3}
    # Mutating the source later must not affect the merged dict.
    b.tool_names["Glob"] = 9
    assert "Glob" not in a.tool_names


def test_content_breakdown_clone_isolates_tool_names() -> None:
    from bnf.proxy.parser import ContentBreakdown

    a = ContentBreakdown(tool_names={"Read": 1})
    b = a.clone()
    b.tool_names["Grep"] = 5
    assert "Grep" not in a.tool_names


def test_anthropic_tool_counts_from_request_body() -> None:
    body = json.dumps(
        {
            "messages": [
                {
                    "role": "assistant",
                    "content": [
                        {"type": "tool_use", "id": "t1", "name": "Read", "input": {"p": "a"}},
                        {"type": "tool_use", "id": "t2", "name": "Read", "input": {"p": "b"}},
                    ],
                },
                {
                    "role": "user",
                    "content": [
                        {"type": "tool_result", "tool_use_id": "t1", "content": "A"},
                        {"type": "tool_result", "tool_use_id": "t2", "content": "B"},
                    ],
                },
            ]
        }
    ).encode()
    br = analyze_request_body(body, "anthropic")
    assert br.tool_call_count == 2
    assert br.tool_result_count == 2


def test_openai_developer_role_counted_as_system_prompt() -> None:
    """Codex puts sandbox / permissions instructions in a ``developer``
    role message. That content is harness-imposed, not user-authored,
    so it belongs in ``system_prompt_bytes`` — same bucket as the
    top-level ``instructions`` field — not ``user_text_bytes``. And
    wire + novel must agree on the bucket so novel ≤ wire holds per
    field.
    """

    body_sample = json.dumps(
        {
            "input": [
                {
                    "role": "developer",
                    "content": [{"type": "input_text", "text": "<permissions…>"}],
                },
                {
                    "role": "user",
                    "content": [{"type": "input_text", "text": "hi"}],
                },
            ]
        }
    ).encode()

    wire = analyze_request_body(body_sample, "openai")
    assert wire.system_prompt_bytes == len(b"<permissions\xe2\x80\xa6>")
    assert wire.user_text_bytes == 2
    assert wire.assistant_text_bytes == 0

    tracker = NovelTracker()
    novel = analyze_request_body_novel(body_sample, "openai", tracker)
    assert novel.system_prompt_bytes == wire.system_prompt_bytes
    assert novel.user_text_bytes == wire.user_text_bytes
    assert novel.assistant_text_bytes == 0


def test_openai_tool_counts_from_request_body() -> None:
    body = json.dumps(
        {
            "input": [
                {"type": "function_call", "name": "sh", "arguments": "{}", "call_id": "c1"},
                {"type": "function_call", "name": "sh", "arguments": "{}", "call_id": "c2"},
                {"type": "function_call_output", "call_id": "c1", "output": "A"},
                {"type": "function_call_output", "call_id": "c2", "output": "B"},
            ]
        }
    ).encode()
    br = analyze_request_body(body, "openai")
    assert br.tool_call_count == 2
    assert br.tool_result_count == 2


def test_anthropic_response_stream_counts_tool_calls() -> None:
    analyzer = StreamContentAnalyzer("anthropic")

    def send(obj: dict) -> None:
        analyzer.feed(f"data: {json.dumps(obj)}\n\n".encode())

    send({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
    send({"type": "content_block_stop", "index": 0})
    send(
        {
            "type": "content_block_start",
            "index": 1,
            "content_block": {"type": "tool_use", "id": "t1", "name": "Read", "input": {}},
        }
    )
    send({"type": "content_block_stop", "index": 1})
    send(
        {
            "type": "content_block_start",
            "index": 2,
            "content_block": {"type": "tool_use", "id": "t2", "name": "Grep", "input": {}},
        }
    )
    send({"type": "content_block_stop", "index": 2})
    analyzer.finalize()
    assert analyzer.breakdown.tool_call_count == 2


def test_openai_response_stream_counts_function_calls() -> None:
    analyzer = StreamContentAnalyzer("openai")

    def send(obj: dict) -> None:
        analyzer.feed(f"data: {json.dumps(obj)}\n\n".encode())

    send({"type": "response.output_item.added", "item": {"type": "message", "id": "m1"}})
    send(
        {
            "type": "response.output_item.added",
            "item": {"type": "function_call", "id": "fc1", "name": "sh"},
        }
    )
    send(
        {
            "type": "response.output_item.added",
            "item": {"type": "function_call", "id": "fc2", "name": "sh"},
        }
    )
    send({"type": "response.output_item.added", "item": {"type": "reasoning", "id": "r1"}})
    analyzer.finalize()
    assert analyzer.breakdown.tool_call_count == 2


# ---------------------------------------------------------------------------
# Novel-vs-wire dedup
# ---------------------------------------------------------------------------


def test_novel_anthropic_dedupes_replayed_history() -> None:
    """Simulate a 3-turn session: each turn replays prior tool_use + tool_result.
    Novel tracker should count each item exactly once across the session."""

    tracker = NovelTracker()

    # Turn 1 request — nothing yet; just user prompt + tools + system.
    req1 = json.dumps(
        {
            "system": "S",
            "tools": [{"name": "Read", "description": "reads"}],
            "messages": [{"role": "user", "content": "hi"}],
        }
    ).encode()

    # Turn 2 request — replays turn 1, adds assistant tool_use + tool_result.
    req2 = json.dumps(
        {
            "system": "S",
            "tools": [{"name": "Read", "description": "reads"}],
            "messages": [
                {"role": "user", "content": "hi"},
                {
                    "role": "assistant",
                    "content": [
                        {"type": "tool_use", "id": "t1", "name": "Read", "input": {"p": "a"}}
                    ],
                },
                {
                    "role": "user",
                    "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "FILE_A"}],
                },
            ],
        }
    ).encode()

    # Turn 3 request — replays all of the above, adds a new tool_use + tool_result.
    req3 = json.dumps(
        {
            "system": "S",
            "tools": [{"name": "Read", "description": "reads"}],
            "messages": [
                {"role": "user", "content": "hi"},
                {
                    "role": "assistant",
                    "content": [
                        {"type": "tool_use", "id": "t1", "name": "Read", "input": {"p": "a"}},
                        {"type": "tool_use", "id": "t2", "name": "Read", "input": {"p": "b"}},
                    ],
                },
                {
                    "role": "user",
                    "content": [
                        {"type": "tool_result", "tool_use_id": "t1", "content": "FILE_A"},
                        {"type": "tool_result", "tool_use_id": "t2", "content": "FILE_B"},
                    ],
                },
            ],
        }
    ).encode()

    novel1 = analyze_request_body_novel(req1, "anthropic", tracker)
    novel2 = analyze_request_body_novel(req2, "anthropic", tracker)
    novel3 = analyze_request_body_novel(req3, "anthropic", tracker)

    # Turn 1: system + tools + one user text → all novel.
    assert novel1.system_prompt_bytes == 1  # "S"
    assert novel1.tool_schema_bytes > 0
    assert novel1.user_text_bytes == 2  # "hi"
    assert novel1.tool_result_count == 0
    assert novel1.tool_call_count == 0

    # Turn 2: system / tools / user are replays. tool_use t1 + tool_result t1 are new.
    assert novel2.system_prompt_bytes == 0
    assert novel2.tool_schema_bytes == 0
    assert novel2.user_text_bytes == 0
    assert novel2.tool_call_count == 1
    assert novel2.tool_result_count == 1
    assert novel2.tool_result_bytes == len(b"FILE_A")

    # Turn 3: t1 is a replay, t2 is new.
    assert novel3.tool_call_count == 1
    assert novel3.tool_result_count == 1
    assert novel3.tool_result_bytes == len(b"FILE_B")

    # Sum across the session equals unique work done.
    total_novel_tool_results = (
        novel1.tool_result_count + novel2.tool_result_count + novel3.tool_result_count
    )
    assert total_novel_tool_results == 2


def test_novel_openai_dedupes_replayed_function_calls() -> None:
    tracker = NovelTracker()

    req1 = json.dumps(
        {
            "instructions": "I",
            "tools": [{"type": "function", "name": "sh"}],
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "hello"}]},
            ],
        }
    ).encode()
    req2 = json.dumps(
        {
            "instructions": "I",
            "tools": [{"type": "function", "name": "sh"}],
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "hello"}]},
                {"type": "function_call", "name": "sh", "call_id": "c1", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1", "output": "done"},
            ],
        }
    ).encode()
    req3 = json.dumps(
        {
            "instructions": "I",
            "tools": [{"type": "function", "name": "sh"}],
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "hello"}]},
                {"type": "function_call", "name": "sh", "call_id": "c1", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1", "output": "done"},
                {"type": "function_call", "name": "sh", "call_id": "c2", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c2", "output": "again"},
            ],
        }
    ).encode()

    n1 = analyze_request_body_novel(req1, "openai", tracker)
    n2 = analyze_request_body_novel(req2, "openai", tracker)
    n3 = analyze_request_body_novel(req3, "openai", tracker)

    assert n1.system_prompt_bytes == 1  # "I"
    assert n1.user_text_bytes == 5
    assert n1.tool_call_count == 0

    assert n2.user_text_bytes == 0  # replayed
    assert n2.tool_call_count == 1
    assert n2.tool_result_count == 1

    assert n3.tool_call_count == 1
    assert n3.tool_result_count == 1
    assert n3.tool_result_bytes == len(b"again")
