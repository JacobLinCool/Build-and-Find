"""Registry of built-in agent adapters.

Each final-study model with a verified effort knob is registered twice:

* ``<id>_high`` — same model with ``reasoning_effort="high"``;
* ``<id>_low`` — same model with ``reasoning_effort="low"``.

For codex this is wired via ``-c model_reasoning_effort="<level>"``; for
claude-code via the ``--effort <level>`` flag.

New adapters can be added at runtime via :func:`register`.
"""

from __future__ import annotations

from bnf.runners.base import AgentAdapter
from bnf.runners.codex import CodexAdapter
from bnf.runners.claude_code import ClaudeCodeAdapter

_EFFORT_VARIANTS: tuple[str, ...] = ("high", "low")


def _variants(
    base_id: str,
    factory,
) -> dict[str, AgentAdapter]:
    out: dict[str, AgentAdapter] = {}
    for effort in _EFFORT_VARIANTS:
        agent_id = base_id if effort is None else f"{base_id}_{effort}"
        out[agent_id] = factory(agent_id, effort)
    return out


def _default_registry() -> dict[str, AgentAdapter]:
    registry: dict[str, AgentAdapter] = {}
    codex_models = [
        ("codex_gpt5_5", "gpt-5.5"),
        ("codex_gpt5_4", "gpt-5.4"),
        ("codex_gpt5_4_mini", "gpt-5.4-mini"),
    ]
    claude_models = [
        ("claude_code_opus_4_7", "claude-opus-4-7"),
        ("claude_code_opus_4_6", "claude-opus-4-6"),
        ("claude_code_sonnet_4_6", "claude-sonnet-4-6"),
    ]
    for base_id, model_id in codex_models:
        registry.update(
            _variants(
                base_id,
                lambda aid, eff, mid=model_id: CodexAdapter(
                    id=aid, model_id=mid, reasoning_effort=eff
                ),
            )
        )
    for base_id, model_id in claude_models:
        registry.update(
            _variants(
                base_id,
                lambda aid, eff, mid=model_id: ClaudeCodeAdapter(
                    id=aid, model_id=mid, reasoning_effort=eff
                ),
            )
        )
    # Third-party Anthropic-compatible providers driving the same
    # claude-code CLI. MiMo receives the same explicit high/low effort
    # flag as first-party Claude Code; provider behavior is verified by
    # smoke runs before the full experiment.
    third_party_claude = [
        ("claude_code_mimo_v2_5_pro", "mimo-v2.5-pro", "xiaomimimo", "MIMO_API_KEY"),
        ("claude_code_mimo_v2_5", "mimo-v2.5", "xiaomimimo", "MIMO_API_KEY"),
    ]
    for base_id, model_id, upstream, env_var in third_party_claude:
        registry.update(
            _variants(
                base_id,
                lambda aid, eff, mid=model_id, up=upstream, auth=env_var: ClaudeCodeAdapter(
                    id=aid,
                    model_id=mid,
                    upstream=up,
                    auth_token_env=auth,
                    reasoning_effort=eff,
                ),
            )
        )
    return registry


AGENT_REGISTRY: dict[str, AgentAdapter] = _default_registry()


def register(adapter: AgentAdapter) -> None:
    AGENT_REGISTRY[adapter.id] = adapter


def get_agent(agent_id: str) -> AgentAdapter:
    if agent_id not in AGENT_REGISTRY:
        raise KeyError(f"unknown agent id {agent_id!r}; available: {sorted(AGENT_REGISTRY)}")
    return AGENT_REGISTRY[agent_id]


def list_agents() -> list[AgentAdapter]:
    return list(AGENT_REGISTRY.values())
