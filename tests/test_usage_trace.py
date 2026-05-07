from __future__ import annotations

import json

from bnf.runners.base import AgentInvocation
from bnf.runners.usage_trace import collect_claude_usage, collect_codex_usage


def test_collect_codex_usage_from_session_trace(tmp_path, monkeypatch) -> None:
    codex_home = tmp_path / ".codex"
    session_dir = codex_home / "sessions" / "2026" / "04" / "22"
    session_dir.mkdir(parents=True)
    monkeypatch.setenv("CODEX_HOME", str(codex_home))

    thread_id = "019-test-thread"
    trace_path = session_dir / f"rollout-2026-04-22T00-00-00-{thread_id}.jsonl"
    trace_path.write_text(
        "\n".join(
            [
                json.dumps(
                    {
                        "type": "response_item",
                        "payload": {
                            "type": "message",
                            "role": "assistant",
                            "phase": "commentary",
                            "content": [{"type": "output_text", "text": "Inspecting"}],
                        },
                    }
                ),
                json.dumps(
                    {
                        "type": "response_item",
                        "payload": {
                            "type": "function_call",
                            "name": "exec_command",
                            "call_id": "call_1",
                            "arguments": "ls",
                        },
                    }
                ),
                json.dumps(
                    {
                        "type": "response_item",
                        "payload": {
                            "type": "function_call_output",
                            "call_id": "call_1",
                            "output": "alpha\nbeta\n",
                        },
                    }
                ),
                json.dumps(
                    {
                        "type": "response_item",
                        "payload": {
                            "type": "message",
                            "role": "assistant",
                            "phase": "final_answer",
                            "content": [{"type": "output_text", "text": "Done"}],
                        },
                    }
                ),
                json.dumps(
                    {
                        "type": "event_msg",
                        "payload": {
                            "type": "token_count",
                            "info": {
                                "total_token_usage": {
                                    "input_tokens": 100,
                                    "cached_input_tokens": 40,
                                    "output_tokens": 12,
                                    "reasoning_output_tokens": 7,
                                    "total_tokens": 112,
                                }
                            },
                        },
                    }
                ),
            ]
        ),
        encoding="utf-8",
    )

    stdout = "\n".join(
        [
            json.dumps({"type": "thread.started", "thread_id": thread_id}),
            json.dumps({"type": "turn.started"}),
        ]
    )
    inv = AgentInvocation(prompt="question", workspace=tmp_path, timeout_s=30)
    collected = collect_codex_usage(inv, stdout, "", 1.25)

    assert collected.usage.usage_source == "session_log"
    assert collected.usage.session_id == thread_id
    assert collected.usage.trace_path == str(trace_path)
    assert collected.usage.prompt_bytes == len("question".encode("utf-8"))
    assert collected.usage.retrieval_payload_bytes == len("alpha\nbeta\n".encode("utf-8"))
    assert collected.usage.tool_input_bytes == len("ls".encode("utf-8"))
    assert collected.usage.assistant_message_bytes == len("InspectingDone".encode("utf-8"))
    assert collected.usage.final_answer_bytes == len("Done".encode("utf-8"))
    assert collected.usage.tool_calls == 1
    assert collected.usage.vendor_usage is not None
    assert collected.usage.vendor_usage.total_tokens == 112
    assert collected.usage.vendor_usage.reasoning_output_tokens == 7


def test_collect_claude_usage_from_project_trace(tmp_path, monkeypatch) -> None:
    monkeypatch.setenv("HOME", str(tmp_path))
    claude_projects = tmp_path / ".claude" / "projects" / "project-slug"
    claude_projects.mkdir(parents=True)

    session_id = "session-123"
    trace_path = claude_projects / f"{session_id}.jsonl"
    trace_path.write_text(
        "\n".join(
            [
                json.dumps(
                    {
                        "type": "assistant",
                        "requestId": "req-1",
                        "sessionId": session_id,
                        "message": {
                            "usage": {
                                "input_tokens": 3,
                                "cache_creation_input_tokens": 0,
                                "cache_read_input_tokens": 10,
                                "output_tokens": 1,
                            },
                            "content": [{"type": "text", "text": "Inspecting"}],
                        },
                    }
                ),
                json.dumps(
                    {
                        "type": "assistant",
                        "requestId": "req-1",
                        "sessionId": session_id,
                        "message": {
                            "usage": {
                                "input_tokens": 3,
                                "cache_creation_input_tokens": 0,
                                "cache_read_input_tokens": 10,
                                "output_tokens": 9,
                            },
                            "content": [
                                {
                                    "type": "tool_use",
                                    "id": "toolu_1",
                                    "name": "Read",
                                    "input": {"file_path": "/tmp/example.rs"},
                                }
                            ],
                        },
                    }
                ),
                json.dumps(
                    {
                        "type": "user",
                        "sessionId": session_id,
                        "message": {
                            "content": [
                                {
                                    "type": "tool_result",
                                    "tool_use_id": "toolu_1",
                                    "content": "fn main() {}\n",
                                }
                            ]
                        },
                    }
                ),
                json.dumps(
                    {
                        "type": "assistant",
                        "requestId": "req-2",
                        "sessionId": session_id,
                        "message": {
                            "usage": {
                                "input_tokens": 5,
                                "cache_creation_input_tokens": 20,
                                "cache_read_input_tokens": 30,
                                "output_tokens": 4,
                            },
                            "content": [{"type": "text", "text": "Answer"}],
                        },
                    }
                ),
            ]
        ),
        encoding="utf-8",
    )

    stdout = "\n".join(
        [
            json.dumps({"type": "system", "session_id": session_id}),
            json.dumps(
                {
                    "type": "result",
                    "session_id": session_id,
                    "result": "Answer",
                    "total_cost_usd": 0.5,
                    "usage": {
                        "input_tokens": 8,
                        "cache_creation_input_tokens": 20,
                        "cache_read_input_tokens": 40,
                        "output_tokens": 13,
                    },
                }
            ),
        ]
    )

    inv = AgentInvocation(prompt="question", workspace=tmp_path, timeout_s=30)
    collected = collect_claude_usage(inv, stdout, "", 2.5)

    assert collected.usage.usage_source == "project_log"
    assert collected.usage.session_id == session_id
    assert collected.usage.trace_path == str(trace_path)
    assert collected.usage.prompt_bytes == len("question".encode("utf-8"))
    assert collected.usage.retrieval_payload_bytes == len("fn main() {}\n".encode("utf-8"))
    assert collected.usage.tool_input_bytes == len("/tmp/example.rs".encode("utf-8"))
    assert collected.usage.assistant_message_bytes == len("InspectingAnswer".encode("utf-8"))
    assert collected.usage.final_answer_bytes == len("Answer".encode("utf-8"))
    assert collected.usage.tool_calls == 1
    assert collected.usage.vendor_usage is not None
    assert collected.usage.vendor_usage.input_tokens == 8
    assert collected.usage.vendor_usage.cache_creation_input_tokens == 20
    assert collected.usage.vendor_usage.cache_read_input_tokens == 40
    assert collected.usage.vendor_usage.output_tokens == 13
    assert collected.usage.vendor_usage.total_tokens == 81
    assert collected.usage.vendor_usage.cost_usd == 0.5
