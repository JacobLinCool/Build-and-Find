"""Replay-based regression tests.

Deterministic, zero-cost checks that the proxy's parsing, novel
dedup, turn detection, and slice logic produce stable numbers against
hand-crafted synthetic fixtures (useful for multi-turn scenarios we
can't easily capture from live APIs).
"""

from __future__ import annotations

from ._replay import (
    make_synthetic_anthropic_fixture,
    replay_context,
)


def test_synthetic_multi_turn_yields_two_turns() -> None:
    """Hand-crafted two-prompt session. The proxy must detect the
    turn boundary at the event that introduces the second user
    prompt — right after the first turn's answer."""

    turn1_tool_use = {
        "content": [{"type": "tool_use", "id": "T1", "name": "Read", "input": {}}],
        "usage": {"input_tokens": 100, "output_tokens": 5},
    }
    turn1_answer = {
        "content": [{"type": "text", "text": "answer for prompt one"}],
        "usage": {"input_tokens": 120, "output_tokens": 15},
    }
    turn2_tool_use = {
        "content": [{"type": "tool_use", "id": "T3", "name": "Read", "input": {}}],
        "usage": {"input_tokens": 150, "output_tokens": 5},
    }
    turn2_answer = {
        "content": [{"type": "text", "text": "answer for prompt two"}],
        "usage": {"input_tokens": 180, "output_tokens": 20},
    }

    turns = [
        # Turn 1 bootstrap
        {
            "request": {"messages": [{"role": "user", "content": "prompt one"}]},
            "response": turn1_tool_use,
        },
        # Turn 1 loop (has tool_result)
        {
            "request": {
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
            },
            "response": turn1_answer,
        },
        # Turn 2 bootstrap: replays turn 1's full history + fresh "prompt two".
        {
            "request": {
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
                    {
                        "role": "assistant",
                        "content": [{"type": "text", "text": "answer for prompt one"}],
                    },
                    {"role": "user", "content": "prompt two"},
                ]
            },
            "response": turn2_tool_use,
        },
        # Turn 2 loop: full history + new tool_use + new tool_result.
        {
            "request": {
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
                    {
                        "role": "assistant",
                        "content": [{"type": "text", "text": "answer for prompt one"}],
                    },
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
            },
            "response": turn2_answer,
        },
    ]
    events = make_synthetic_anthropic_fixture(turns)

    with replay_context(events) as ctx:
        stats = ctx.drive()

    # Two turns, each with 2 events (bootstrap + answer).
    assert stats.turn_start_indices == [0, 2]
    assert len(stats.turns) == 2
    assert [len(t.events) for t in stats.turns] == [2, 2]

    # Per-turn splits are independent.
    t0, t1 = stats.turns
    assert t0.loop_start_index == 0  # response 0 is tool_use
    assert t0.driven.event_count == 1
    assert t0.final.event_count == 1
    assert t1.loop_start_index == 0
    assert t1.driven.event_count == 1
    assert t1.final.event_count == 1

    # Session-wide novel dedup still holds: only 2 unique tool_use
    # invocations (T1, T3) though wire history replays them.
    assert stats.novel_content_to_model.tool_call_count == 2
    assert stats.novel_content_to_model.tool_result_count == 2
    # Wire counts inflate from replays (turn 2 replays turn 1's
    # tool_use/result once).
    assert stats.content_to_model.tool_call_count > 2
