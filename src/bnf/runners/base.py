"""Agent-adapter abstraction.

Every concrete CLI-based coding agent (codex, claude-code, future others)
implements :class:`AgentAdapter`. The adapter owns:

1. Launching the agent inside the isolated workspace.
2. Feeding it the prompt and any environment it needs.
3. Collecting bytes-to-model, bytes-from-model, tool call counts, etc.

For the first implementation we run each agent via subprocess and compute
bytes/tool-calls from the CLI's own output logs where possible, falling back
to crude I/O byte counts otherwise. These are approximations; section 17.3
of the design doc accepts byte counts as a model-agnostic proxy, not a
precise token count.
"""

from __future__ import annotations

import abc
import dataclasses
import os
import shutil
import subprocess
import time
from pathlib import Path
from typing import TYPE_CHECKING, Optional

from bnf.runners.usage_trace import UsageCollection
from bnf.schema.run import Efficiency, UsageEvent, UsageMetrics, VendorTokens

if TYPE_CHECKING:
    from bnf.proxy import Gateway, SessionHandle


def _capture_efficiency(
    gateway: "Gateway | None", session: "SessionHandle | None"
) -> Efficiency | None:
    """Translate a proxy ``SessionStats`` into the research-facing
    :class:`Efficiency` block. Returns ``None`` if no gateway was used.
    """

    if gateway is None or session is None:
        return None
    try:
        stats = gateway.get_stats(session.id)
    except Exception:
        # Proxy state is best-effort — never fail the whole run just
        # because stats extraction hiccupped.
        return None
    # Compute tok/s from session totals (paper §17.3).
    gen_seconds = sum((e.generation_seconds or 0) for e in stats.events)
    out_tokens = stats.vendor.non_reasoning_output_tokens + stats.vendor.reasoning_output_tokens
    tok_per_s = (out_tokens / gen_seconds) if gen_seconds > 0 else None
    nc = stats.novel_content_to_model
    return Efficiency(
        novel_retrieval_bytes=nc.tool_result_bytes,
        novel_tool_call_bytes=stats.content_from_model.tool_call_bytes,
        novel_tool_call_count=stats.content_from_model.tool_call_count,
        novel_tool_result_count=nc.tool_result_count,
        wall_clock_seconds=stats.wall_clock_seconds,
        generation_tok_per_s=tok_per_s,
        tool_names=dict(stats.content_from_model.tool_names),
        turn_count=len(stats.turns),
        vendor_tokens=VendorTokens(
            total_input_tokens=stats.vendor.total_input_tokens,
            cached_input_tokens=stats.vendor.cached_input_tokens,
            uncached_input_tokens=stats.vendor.uncached_input_tokens,
            cache_creation_input_tokens=stats.vendor.cache_creation_input_tokens,
            non_reasoning_output_tokens=stats.vendor.non_reasoning_output_tokens,
            reasoning_output_tokens=stats.vendor.reasoning_output_tokens,
            total_tokens=stats.vendor.total_tokens,
        ),
        proxy_session_id=stats.id,
        request_count=stats.requests,
        raw_request_bytes=stats.raw_request_bytes,
        raw_response_bytes=stats.raw_response_bytes,
    )


@dataclasses.dataclass
class AgentInvocation:
    """Everything the adapter needs to run the agent once."""

    prompt: str
    workspace: Path
    timeout_s: int | None
    env: dict[str, str] = dataclasses.field(default_factory=dict)
    # Extra, adapter-specific knobs (e.g. non-interactive flag, headless mode).
    extra: dict[str, str] = dataclasses.field(default_factory=dict)


@dataclasses.dataclass
class IsolatedHome:
    """An ephemeral config directory for one agent run.

    The adapter populates ``path`` with whatever auth material the agent
    needs (e.g. a freshly extracted OAuth credential), and ``env_overrides``
    with the env vars that point the agent CLI at it (``CLAUDE_CONFIG_DIR``
    / ``CODEX_HOME``). The base runner removes ``path`` after usage
    collection; until then session logs persisted there are still readable.
    """

    path: Path
    env_overrides: dict[str, str] = dataclasses.field(default_factory=dict)


@dataclasses.dataclass
class AgentResult:
    status: str  # "ok" | "error" | "timeout"
    stdout: str
    stderr: str
    usage: UsageMetrics
    usage_events: list[UsageEvent] = dataclasses.field(default_factory=list)
    error: Optional[str] = None


_INFRASTRUCTURE_FAILURE_PATTERNS: tuple[tuple[str, str], ...] = (
    ("authentication failed", "agent authentication failed"),
    ("authentication_failed", "agent authentication failed"),
    ("failed to authenticate", "agent authentication failed"),
    ("invalid authentication credentials", "agent authentication failed"),
    ("not logged in", "agent authentication failed"),
    ("please run /login", "agent authentication failed"),
    ("insufficient_quota", "provider quota exhausted"),
    ("quota", "provider quota exhausted"),
    ("usage limit", "provider usage limit reached"),
    ("rate_limit", "provider rate limit reached"),
    ("rate limit", "provider rate limit reached"),
    ("overloaded_error", "provider overloaded"),
)


def detect_infrastructure_failure(result: AgentResult) -> str | None:
    """Classify provider/CLI failures that should abort a batch run.

    A model producing a bad or uncompilable artifact is experimental data.
    A CLI returning a synthetic auth/quota/rate-limit message before the
    model can use tools is not; continuing would turn one provider outage
    into many misleading failed cells.
    """

    if result.status == "ok":
        return None
    haystack = "\n".join(
        part for part in (result.error or "", result.stdout, result.stderr) if part
    ).lower()
    for needle, reason in _INFRASTRUCTURE_FAILURE_PATTERNS:
        if needle in haystack:
            return reason
    return None


class AgentAdapter(abc.ABC):
    """Common interface for all coding-agent adapters."""

    #: Stable identifier used in run records and CLI flags.
    id: str
    #: User-facing description.
    description: str
    #: The agent CLI that backs this adapter (e.g., ``codex``).
    cli_bin: str
    #: Model identifier passed through to the agent CLI.
    model_id: str
    #: ``codex`` or ``claude_code``; determines logging/parsing heuristics.
    family: str
    #: Optional reasoning effort level (``"low"``, ``"medium"``, ``"high"``).
    #: ``None`` means the underlying CLI's default is used.
    reasoning_effort: str | None

    def __init__(
        self,
        *,
        id: str,
        model_id: str,
        cli_bin: str,
        description: str = "",
        reasoning_effort: str | None = None,
    ) -> None:
        self.id = id
        self.model_id = model_id
        self.cli_bin = cli_bin
        self.reasoning_effort = reasoning_effort
        self.description = description or f"{self.family} adapter for {model_id}"

    # ------------------------------------------------------------------
    # Required API
    # ------------------------------------------------------------------

    @abc.abstractmethod
    def build_argv(self, inv: AgentInvocation) -> list[str]:
        """Return argv for the agent subprocess."""

    def wire_proxy(
        self, inv: AgentInvocation, session: "SessionHandle"
    ) -> tuple[list[str], dict[str, str]]:
        """Translate a proxy session into the env + argv overrides this
        agent needs so it talks to the proxy instead of the real vendor.

        Returns ``(extra_argv, env_updates)``. The default implementation
        is a no-op — subclasses override to inject vendor-specific
        env vars (``ANTHROPIC_BASE_URL`` / ``OPENAI_BASE_URL``) or
        config flags.
        """

        del inv, session  # unused in the no-op default
        return [], {}

    # ------------------------------------------------------------------
    # Shared helpers
    # ------------------------------------------------------------------

    def ensure_available(self) -> None:
        if shutil.which(self.cli_bin) is None:
            raise RuntimeError(
                f"Agent CLI {self.cli_bin!r} not found on PATH for adapter {self.id!r}."
            )

    def _prepare_isolated_home(self, inv: AgentInvocation) -> IsolatedHome | None:
        """Build a fresh, ephemeral config home for this run.

        Override in concrete adapters to materialise the agent's auth
        credentials inside a tempdir and return it. Returning ``None`` keeps
        the agent on the user's real config home (e.g. for adapters that
        have no per-user state to isolate).
        """

        del inv
        return None

    def run(
        self,
        inv: AgentInvocation,
        *,
        gateway: "Gateway | None" = None,
    ) -> AgentResult:
        """Execute the agent. Dry-run capable if the adapter overrides.

        Implementation notes
        --------------------
        * The prompt is passed via stdin. Agents that prefer a flag should
          override ``run`` or ``build_argv``.
        * We gate on ``BNF_DRY_RUN=1`` to enable deterministic offline
          smoke tests without spawning the agent (see
          :class:`EchoAgentAdapter`).
        * If ``gateway`` is provided, a fresh proxy session is created
          and the agent is pointed at it via adapter-specific env/config
          (see :meth:`wire_proxy`). Stats are read back from the session
          after the subprocess exits and attached to
          ``AgentResult.usage.efficiency``.
        """

        if os.environ.get("BNF_DRY_RUN") == "1":
            return self._dry_run(inv)
        self.ensure_available()
        argv = self.build_argv(inv)
        env = os.environ.copy()
        env.update(inv.env)

        isolated_home: IsolatedHome | None = None
        if os.environ.get("BNF_NO_ISOLATION") != "1":
            isolated_home = self._prepare_isolated_home(inv)
            if isolated_home is not None:
                env.update(isolated_home.env_overrides)

        try:
            proxy_session = None
            if gateway is not None:
                proxy_session = gateway.create_session(upstream=self._proxy_upstream())
                extra_argv, env_updates = self.wire_proxy(inv, proxy_session)
                argv = argv + list(extra_argv)
                env.update(env_updates)
            start = time.monotonic()
            try:
                proc = subprocess.run(
                    argv,
                    input=inv.prompt,
                    capture_output=True,
                    text=True,
                    cwd=str(inv.workspace),
                    env=env,
                    timeout=inv.timeout_s,
                    check=False,
                )
            except subprocess.TimeoutExpired as exc:
                elapsed = time.monotonic() - start
                usage = UsageMetrics(wall_clock_seconds=elapsed)
                usage.efficiency = _capture_efficiency(gateway, proxy_session)
                return AgentResult(
                    status="timeout",
                    stdout=(exc.stdout or b"").decode("utf-8", errors="replace")
                    if isinstance(exc.stdout, (bytes, bytearray))
                    else (exc.stdout or ""),
                    stderr=(exc.stderr or b"").decode("utf-8", errors="replace")
                    if isinstance(exc.stderr, (bytes, bytearray))
                    else (exc.stderr or ""),
                    usage=usage,
                    error=f"timeout after {inv.timeout_s}s",
                )
            elapsed = time.monotonic() - start
            status = "ok" if proc.returncode == 0 else "error"
            try:
                collected = self._collect_usage(
                    inv,
                    proc.stdout,
                    proc.stderr,
                    elapsed,
                    isolated_home=isolated_home,
                )
            except Exception as exc:
                prompt_bytes = len(inv.prompt.encode("utf-8"))
                error = f"usage collection failed: {exc}"
                if status == "error":
                    error = f"exit code {proc.returncode}; {error}"
                usage = UsageMetrics(
                    wall_clock_seconds=elapsed,
                    prompt_bytes=prompt_bytes,
                    bytes_to_model=prompt_bytes,
                )
                usage.efficiency = _capture_efficiency(gateway, proxy_session)
                return AgentResult(
                    status="error",
                    stdout=proc.stdout,
                    stderr=proc.stderr,
                    usage=usage,
                    usage_events=[],
                    error=error,
                )
            usage = collected.usage
            usage.efficiency = _capture_efficiency(gateway, proxy_session)
            return AgentResult(
                status=status,
                stdout=proc.stdout,
                stderr=proc.stderr,
                usage=usage,
                usage_events=collected.events,
                error=None if status == "ok" else f"exit code {proc.returncode}",
            )
        finally:
            if isolated_home is not None:
                shutil.rmtree(isolated_home.path, ignore_errors=True)

    def _proxy_upstream(self) -> str:
        """Name of the proxy upstream this adapter talks to. Override
        in codex when running in ChatGPT-login mode."""

        return "anthropic" if self.family == "claude_code" else "openai"

    def _collect_usage(
        self,
        inv: AgentInvocation,
        stdout: str,
        stderr: str,
        elapsed: float,
        *,
        isolated_home: IsolatedHome | None = None,
    ) -> UsageCollection:
        """Default byte counting for adapters without a structured stream.

        Concrete adapters override this to count UTF-8 bytes of the
        text actually exchanged with the model. The base implementation
        is a placeholder — it records only what is directly visible (the
        prompt we sent). Token-to-byte inference is never used.
        """

        del isolated_home  # default impl ignores it
        prompt_bytes = len(inv.prompt.encode("utf-8"))
        return UsageCollection(
            usage=UsageMetrics(
                wall_clock_seconds=elapsed,
                prompt_bytes=prompt_bytes,
                bytes_to_model=prompt_bytes,
                bytes_from_model=0,
                tool_calls=0,
                network_requests=0,
            ),
            events=[],
        )

    def _dry_run(self, inv: AgentInvocation) -> AgentResult:
        """Offline smoke-test mode.

        Produces a minimal build artefact (a stub Rust project) or, for
        the Find phase, writes a deterministic ``find_answers.json`` so
        the scoring pipeline has something to consume. Real benchmark
        runs MUST NOT set ``BNF_DRY_RUN=1``.
        """

        import json
        import re

        inv.workspace.mkdir(parents=True, exist_ok=True)
        is_find_phase = "Question " in inv.prompt and "find_answers.json" in inv.prompt
        if is_find_phase:
            qids = re.findall(r"Question\s+(\S+)", inv.prompt)
            answers = [{"question_id": qid, "selected_option": "A"} for qid in qids]
            (inv.workspace / "find_answers.json").write_text(
                json.dumps({"answers": answers}, indent=2), encoding="utf-8"
            )
        else:
            # Minimal Rust project so the validator can exercise file-
            # counting paths even offline.
            (inv.workspace / "Cargo.toml").write_text(
                '[package]\nname = "bnf_dry_run"\nversion = "0.0.0"\nedition = "2021"\n',
                encoding="utf-8",
            )
            src = inv.workspace / "src"
            src.mkdir(exist_ok=True)
            (src / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
        marker = inv.workspace / "BNF_DRY_RUN.txt"
        marker.write_text(
            f"adapter={self.id} model={self.model_id} len(prompt)={len(inv.prompt)}\n",
            encoding="utf-8",
        )
        usage = UsageMetrics(
            wall_clock_seconds=0.0,
            prompt_bytes=len(inv.prompt.encode("utf-8")),
            bytes_to_model=len(inv.prompt.encode("utf-8")),
            bytes_from_model=len(marker.read_bytes()),
            tool_calls=0,
        )
        return AgentResult(
            status="ok",
            stdout="dry-run",
            stderr="",
            usage=usage,
            usage_events=[],
        )
