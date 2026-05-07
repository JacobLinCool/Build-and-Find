"""Integration test for the Gateway + AgentAdapter wiring.

Verifies that when an :class:`AgentAdapter` is handed a
:class:`Gateway`, the adapter creates a proxy session, injects the
right vendor env/config, and reads per-run efficiency back into
``AgentResult.usage.efficiency`` — all without hitting any real vendor
API.

The test replaces the vendor upstream with a
:class:`~tests.proxy._replay.ReplayUpstream` fed from a hand-crafted
fixture, and runs a *mock* adapter that (instead of launching a
subprocess) issues the same HTTP requests the real CLI would. That
isolates the adapter/proxy integration from the CLI-subprocess layer,
which has its own tests.
"""

from __future__ import annotations

import json
import urllib.request
from dataclasses import dataclass

from bnf.proxy import Gateway
from bnf.runners.base import AgentAdapter, AgentInvocation, AgentResult
from bnf.runners.usage_trace import UsageCollection
from bnf.schema.run import UsageMetrics

from ._replay import (
    ReplayUpstream,
    make_synthetic_anthropic_fixture,
)


class _FakeClaudeAdapter(AgentAdapter):
    """Adapter that mimics Claude-Code's control flow: honor
    ANTHROPIC_BASE_URL, issue one POST to /v1/messages per injected
    request body. No subprocess."""

    family = "claude_code"

    def __init__(self, request_bodies: list[bytes]) -> None:
        super().__init__(
            id="fake-claude",
            model_id="fake-opus",
            cli_bin="/bin/true",  # must pass ensure_available
            description="",
        )
        self._bodies = request_bodies

    def build_argv(self, inv: AgentInvocation) -> list[str]:
        return ["/bin/true"]

    def wire_proxy(self, inv, session):
        # Mirror ClaudeCodeAdapter.wire_proxy without inheriting its
        # subprocess plumbing.
        return [], {"ANTHROPIC_BASE_URL": session.base_url}

    def _collect_usage(
        self,
        inv: AgentInvocation,
        stdout: str,
        stderr: str,
        elapsed: float,
        *,
        isolated_home=None,
    ) -> UsageCollection:
        del isolated_home
        return UsageCollection(usage=UsageMetrics(wall_clock_seconds=elapsed))

    def run(self, inv: AgentInvocation, *, gateway=None) -> AgentResult:
        # Replicate the relevant parts of the base class without
        # actually spawning a subprocess: create the proxy session,
        # look up the base URL from ``wire_proxy`` (what the real CLI
        # would see in its environment), drive the recorded request
        # sequence through it, then read ``Efficiency`` back.
        assert gateway is not None, "this fake adapter is only used with a gateway"
        session = gateway.create_session(upstream=self._proxy_upstream())
        _, env_updates = self.wire_proxy(inv, session)
        base_url = env_updates["ANTHROPIC_BASE_URL"]
        for body in self._bodies:
            req = urllib.request.Request(
                f"{base_url}/v1/messages",
                data=body,
                method="POST",
            )
            req.add_header("Content-Type", "application/json")
            with urllib.request.urlopen(req, timeout=10) as resp:
                resp.read()
        usage = UsageMetrics(wall_clock_seconds=0.01)
        from bnf.runners.base import _capture_efficiency

        usage.efficiency = _capture_efficiency(gateway, session)
        return AgentResult(status="ok", stdout="", stderr="", usage=usage)


def test_adapter_populates_efficiency_when_gateway_provided(tmp_path) -> None:
    """End-to-end: gateway + replay upstream + adapter → Efficiency
    block on the AgentResult."""

    turns = [
        {
            "request": {"messages": [{"role": "user", "content": "first prompt"}]},
            "response": {
                "content": [{"type": "tool_use", "id": "T1", "name": "Read", "input": {}}],
                "usage": {"input_tokens": 50, "output_tokens": 5},
            },
        },
        {
            "request": {
                "messages": [
                    {"role": "user", "content": "first prompt"},
                    {
                        "role": "assistant",
                        "content": [{"type": "tool_use", "id": "T1", "name": "Read", "input": {}}],
                    },
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "tool_result",
                                "tool_use_id": "T1",
                                "content": "FILE BYTES HERE",
                            }
                        ],
                    },
                ]
            },
            "response": {
                "content": [{"type": "text", "text": "final answer"}],
                "usage": {"input_tokens": 80, "output_tokens": 10},
            },
        },
    ]
    events = make_synthetic_anthropic_fixture(turns)

    upstream = ReplayUpstream(events)
    upstream.start()
    gw = Gateway.start()
    try:
        # Override the adapter's session to point at the replay upstream
        # rather than real api.anthropic.com. We do this by monkey-
        # patching UPSTREAMS just for this test.
        from bnf.proxy import gateway as gw_module

        original = gw_module.UPSTREAMS["anthropic"]
        gw_module.UPSTREAMS["anthropic"] = upstream.base_url
        try:
            adapter = _FakeClaudeAdapter(
                request_bodies=[json.dumps(t["request"]).encode() for t in turns]
            )
            inv = AgentInvocation(
                prompt="driven by replay",
                workspace=tmp_path,
                timeout_s=10,
            )
            result = adapter.run(inv, gateway=gw)
        finally:
            gw_module.UPSTREAMS["anthropic"] = original
    finally:
        gw.stop()
        upstream.stop()

    assert result.status == "ok"
    eff = result.usage.efficiency
    assert eff is not None, "gateway provided — efficiency must be populated"

    # Core fields from paper §17.3.
    assert eff.novel_retrieval_bytes == len(b"FILE BYTES HERE")
    assert eff.novel_tool_call_count == 1  # model emitted one tool_use (in event 0)
    assert eff.novel_tool_result_count == 1
    assert eff.request_count == 2
    # Tool-name frequency captured from model response.
    assert eff.tool_names == {"Read": 1}
    # Single-prompt session → one turn.
    assert eff.turn_count == 1
    # Vendor tokens still available for within-family analysis.
    assert eff.vendor_tokens is not None
    assert eff.vendor_tokens.total_input_tokens == 50 + 80
    assert eff.vendor_tokens.non_reasoning_output_tokens == 5 + 10


def test_adapter_without_gateway_leaves_efficiency_unset() -> None:
    """When no gateway is supplied, ``efficiency`` stays ``None``."""

    # Run without a gateway — we bypass the HTTP loop entirely by
    # skipping adapter.run and constructing the result directly;
    # what we care about is the schema default.
    # Use _capture_efficiency with no gateway to check the None path.
    from bnf.runners.base import _capture_efficiency

    assert _capture_efficiency(None, None) is None


def test_codex_adapter_chatgpt_mode_wire_proxy(monkeypatch, tmp_path) -> None:
    """Codex in ChatGPT-login mode should inject model_provider
    overrides rather than OPENAI_BASE_URL, and route via the
    ``chatgpt`` upstream."""

    # Point auth.json detection at a fake ChatGPT-mode file.
    fake_home = tmp_path / "home"
    fake_codex = fake_home / ".codex"
    fake_codex.mkdir(parents=True)
    (fake_codex / "auth.json").write_text(
        json.dumps({"auth_mode": "chatgpt", "tokens": {"id_token": "xxx"}}),
        encoding="utf-8",
    )
    monkeypatch.setenv("HOME", str(fake_home))
    # ``Path.home()`` on some platforms caches from HOME at import time;
    # re-import the codex module so the fresh env is seen.
    import importlib

    import bnf.runners.codex as codex_mod

    importlib.reload(codex_mod)

    @dataclass
    class _StubSession:
        id: str = "sid"
        base_url: str = "http://127.0.0.1:9999/s/sid"
        upstream: str = "chatgpt"

    adapter = codex_mod.CodexAdapter(id="codex-x", model_id="gpt-5.4-mini")
    assert adapter._proxy_upstream() == "chatgpt"
    inv = AgentInvocation(prompt="p", workspace=tmp_path, timeout_s=1)
    extra_argv, env_updates = adapter.wire_proxy(inv, _StubSession())
    assert env_updates == {}
    joined = " ".join(extra_argv)
    assert 'model_provider="openai-proxy"' in joined
    assert "http://127.0.0.1:9999/s/sid" in joined
    assert "requires_openai_auth=true" in joined


def test_codex_adapter_apikey_mode_uses_openai_base_url(monkeypatch, tmp_path) -> None:
    fake_home = tmp_path / "home"
    fake_codex = fake_home / ".codex"
    fake_codex.mkdir(parents=True)
    (fake_codex / "auth.json").write_text(
        json.dumps({"auth_mode": "apikey", "OPENAI_API_KEY": "sk-x"}),
        encoding="utf-8",
    )
    monkeypatch.setenv("HOME", str(fake_home))
    import importlib

    import bnf.runners.codex as codex_mod

    importlib.reload(codex_mod)

    @dataclass
    class _StubSession:
        id: str = "sid"
        base_url: str = "http://127.0.0.1:9999/s/sid"
        upstream: str = "openai"

    adapter = codex_mod.CodexAdapter(id="codex-x", model_id="gpt-5.4")
    assert adapter._proxy_upstream() == "openai"
    inv = AgentInvocation(prompt="p", workspace=tmp_path, timeout_s=1)
    extra_argv, env_updates = adapter.wire_proxy(inv, _StubSession())
    assert extra_argv == []
    assert env_updates == {"OPENAI_BASE_URL": "http://127.0.0.1:9999/s/sid/v1"}
