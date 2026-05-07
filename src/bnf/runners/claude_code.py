"""Claude Code CLI adapter."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import TYPE_CHECKING

from bnf.runners.base import AgentAdapter, AgentInvocation, IsolatedHome
from bnf.runners.usage_trace import UsageCollection, collect_claude_usage

if TYPE_CHECKING:
    from bnf.proxy import SessionHandle


class ClaudeCodeAdapter(AgentAdapter):
    """Run the Claude Code CLI against either Anthropic itself or a
    third-party Anthropic-compatible provider (e.g. Xiaomi MiMo).

    For first-party Anthropic, ``upstream='anthropic'`` and the OAuth
    token from the local Keychain / ``.credentials.json`` is reused.

    For a third-party provider, set ``upstream`` to the matching key in
    :data:`bnf.proxy.gateway.UPSTREAMS` and ``auth_token_env`` to the
    name of the env var holding the API key (e.g. ``MIMO_API_KEY``).
    The adapter then injects ``ANTHROPIC_AUTH_TOKEN`` into the agent's
    environment instead of populating ``.credentials.json`` — Claude
    Code prefers the explicit token over the Keychain when set.
    """

    family = "claude_code"

    def __init__(
        self,
        *,
        id: str,
        model_id: str,
        reasoning_effort: str | None = None,
        upstream: str = "anthropic",
        auth_token_env: str | None = None,
    ) -> None:
        suffix = f" (effort={reasoning_effort})" if reasoning_effort else ""
        provider_suffix = f" via {upstream}" if upstream != "anthropic" else ""
        super().__init__(
            id=id,
            model_id=model_id,
            cli_bin="claude",
            description=f"claude-code running model {model_id}{provider_suffix}{suffix}",
            reasoning_effort=reasoning_effort,
        )
        self._upstream = upstream
        self._auth_token_env = auth_token_env

    def build_argv(self, inv: AgentInvocation) -> list[str]:
        argv = [
            self.cli_bin,
            "--print",
            "--model",
            self.model_id,
            "--output-format",
            "stream-json",
            "--verbose",
            "--dangerously-skip-permissions",
        ]
        if self.reasoning_effort is not None:
            argv += ["--effort", self.reasoning_effort]
        return argv

    def _proxy_upstream(self) -> str:
        return self._upstream

    def wire_proxy(
        self, inv: AgentInvocation, session: "SessionHandle"
    ) -> tuple[list[str], dict[str, str]]:
        # Claude Code honors ANTHROPIC_BASE_URL as its API endpoint and
        # forwards whatever auth credential it already holds (OAuth
        # token or API key). The proxy passes that credential through
        # to api.anthropic.com verbatim.
        env: dict[str, str] = {"ANTHROPIC_BASE_URL": session.base_url}
        if self._auth_token_env is not None:
            token = _read_api_key(self._auth_token_env)
            env["ANTHROPIC_AUTH_TOKEN"] = token
        return [], env

    def _prepare_isolated_home(self, inv: AgentInvocation) -> IsolatedHome | None:
        del inv
        home = Path(tempfile.mkdtemp(prefix="bnf-claude-home-"))
        if self._auth_token_env is None:
            creds = _read_claude_credentials()
            target = home / ".credentials.json"
            target.write_text(creds, encoding="utf-8")
            target.chmod(0o600)
        else:
            # Third-party providers authenticate via ANTHROPIC_AUTH_TOKEN
            # (set in wire_proxy), so OAuth credentials are unnecessary.
            # We do still need to mark onboarding as complete so the CLI
            # doesn't drop into an interactive setup flow.
            (home / ".claude.json").write_text(
                json.dumps({"hasCompletedOnboarding": True}), encoding="utf-8"
            )
        return IsolatedHome(path=home, env_overrides={"CLAUDE_CONFIG_DIR": str(home)})

    def _collect_usage(
        self,
        inv: AgentInvocation,
        stdout: str,
        stderr: str,
        elapsed: float,
        *,
        isolated_home: IsolatedHome | None = None,
    ) -> UsageCollection:
        claude_home = isolated_home.path if isolated_home is not None else None
        return collect_claude_usage(
            inv, stdout, stderr, elapsed, claude_home=claude_home
        )


def _read_api_key(env_var: str) -> str:
    """Read a third-party provider API key.

    Looks at ``os.environ`` first, then falls back to a ``.env`` file at
    the repo root (or any ancestor of the cwd) so contributors don't
    have to ``source .env`` before every run. The ``.env`` parser is
    intentionally minimal — bare ``KEY=value`` lines, optional surrounding
    quotes, ``#`` line comments, no shell expansion.
    """

    val = os.environ.get(env_var)
    if val:
        return val
    for d in (Path.cwd(), *Path.cwd().parents):
        env_file = d / ".env"
        if not env_file.is_file():
            continue
        for line in env_file.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, _, value = line.partition("=")
            if key.strip() != env_var:
                continue
            value = value.strip().strip('"').strip("'")
            if value:
                return value
        break
    raise RuntimeError(
        f"{env_var} not set in environment and not found in nearby .env file."
    )


def _read_claude_credentials() -> str:
    """Return Claude Code OAuth credentials JSON as text.

    On macOS the canonical store is the system Keychain entry
    ``Claude Code-credentials``; on other platforms it's
    ``~/.claude/.credentials.json``.
    """

    if sys.platform == "darwin":
        try:
            res = subprocess.run(
                [
                    "security",
                    "find-generic-password",
                    "-s",
                    "Claude Code-credentials",
                    "-a",
                    os.environ.get("USER", ""),
                    "-w",
                ],
                capture_output=True,
                text=True,
                check=True,
                timeout=10,
            )
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
            res = None
        if res is not None and res.stdout.strip():
            return res.stdout.rstrip("\n") + "\n"

    src = Path.home() / ".claude" / ".credentials.json"
    if src.exists():
        return src.read_text(encoding="utf-8")
    raise RuntimeError(
        "Claude Code credentials not found in macOS Keychain "
        "('Claude Code-credentials') or ~/.claude/.credentials.json. "
        "Run `claude` once to log in before running experiments."
    )
