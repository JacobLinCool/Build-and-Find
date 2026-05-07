"""Codex CLI adapter (OpenAI ``codex`` command)."""

from __future__ import annotations

import json
import shutil
import tempfile
from pathlib import Path
from typing import TYPE_CHECKING

from bnf.runners.base import AgentAdapter, AgentInvocation, IsolatedHome
from bnf.runners.usage_trace import UsageCollection, collect_codex_usage

if TYPE_CHECKING:
    from bnf.proxy import SessionHandle


class CodexAdapter(AgentAdapter):
    family = "codex"

    def __init__(
        self, *, id: str, model_id: str, reasoning_effort: str | None = None
    ) -> None:
        suffix = f" (effort={reasoning_effort})" if reasoning_effort else ""
        super().__init__(
            id=id,
            model_id=model_id,
            cli_bin="codex",
            description=f"Codex CLI running model {model_id}{suffix}",
            reasoning_effort=reasoning_effort,
        )

    def build_argv(self, inv: AgentInvocation) -> list[str]:
        argv = [
            self.cli_bin,
            "exec",
            "--model",
            self.model_id,
            "--cd",
            str(inv.workspace),
            "--json",
            "--skip-git-repo-check",
            "--dangerously-bypass-approvals-and-sandbox",
        ]
        if self.reasoning_effort is not None:
            argv += ["-c", f'model_reasoning_effort="{self.reasoning_effort}"']
        return argv

    def _proxy_upstream(self) -> str:
        # ChatGPT-login mode sends to chatgpt.com/backend-api/codex,
        # not api.openai.com. The proxy has a dedicated upstream for
        # that so the response-body sniffing and path rewrites match.
        return "chatgpt" if _detect_auth_mode() == "chatgpt" else "openai"

    def wire_proxy(
        self, inv: AgentInvocation, session: "SessionHandle"
    ) -> tuple[list[str], dict[str, str]]:
        mode = _detect_auth_mode()
        if mode == "chatgpt":
            # The built-in ``openai`` model_provider is reserved, so we
            # register a custom one that keeps ChatGPT auth
            # (``requires_openai_auth=true``) but points its base URL at
            # the proxy. Codex's ``-c`` replaces the whole struct, hence
            # we supply every required field.
            extra = [
                "-c",
                'model_provider="openai-proxy"',
                "-c",
                'model_providers.openai-proxy.name="Build-and-Find Proxy"',
                "-c",
                f'model_providers.openai-proxy.base_url="{session.base_url}"',
                "-c",
                'model_providers.openai-proxy.wire_api="responses"',
                "-c",
                "model_providers.openai-proxy.requires_openai_auth=true",
            ]
            return extra, {}
        # API-key mode: standard OPENAI_BASE_URL with /v1 suffix.
        return [], {"OPENAI_BASE_URL": f"{session.base_url}/v1"}

    def _prepare_isolated_home(self, inv: AgentInvocation) -> IsolatedHome | None:
        del inv
        src = Path.home() / ".codex" / "auth.json"
        if not src.exists():
            raise RuntimeError(
                "~/.codex/auth.json not found. Run `codex` once to log in "
                "before running experiments."
            )
        home = Path(tempfile.mkdtemp(prefix="bnf-codex-home-"))
        target = home / "auth.json"
        shutil.copyfile(src, target)
        target.chmod(0o600)
        return IsolatedHome(path=home, env_overrides={"CODEX_HOME": str(home)})

    def _collect_usage(
        self,
        inv: AgentInvocation,
        stdout: str,
        stderr: str,
        elapsed: float,
        *,
        isolated_home: IsolatedHome | None = None,
    ) -> UsageCollection:
        codex_home = isolated_home.path if isolated_home is not None else None
        return collect_codex_usage(
            inv, stdout, stderr, elapsed, codex_home=codex_home
        )


def _detect_auth_mode() -> str | None:
    """Return ``"chatgpt"`` / ``"apikey"`` / ``None`` from
    ``~/.codex/auth.json``.

    Codex's base URL override mechanism differs between modes, so the
    adapter has to read the local config to choose the right proxy
    wiring.
    """

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
