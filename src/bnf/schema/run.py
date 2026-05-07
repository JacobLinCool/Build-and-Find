"""Run records and reproducibility manifest.

The ``RunManifest`` binds together every identifier we need to reproduce a
run: spec hash, prompt hash, agent/model identifiers, seed, trial id, and an
environment hash (python version, toolchain versions, etc.). Analysis must
be joined on these identifiers so we can detect "identical conditions"
across time.
"""

from __future__ import annotations

import hashlib
import json
import platform
from datetime import datetime, timezone
from pathlib import Path
from typing import Literal

from pydantic import BaseModel, Field


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _env_hash() -> str:
    payload = {
        "python": platform.python_version(),
        "platform": platform.platform(),
        "machine": platform.machine(),
        # Intentionally NOT including locale/time; we only want a reasonably
        # stable identifier across identical machines.
    }
    return hashlib.sha256(json.dumps(payload, sort_keys=True).encode("utf-8")).hexdigest()[:16]


class AgentInfo(BaseModel):
    id: str  # e.g., "codex_gpt5_4", "claude_code_opus_4_7"
    adapter: Literal["codex", "claude_code"]
    model_id: str  # e.g., "gpt-5.4", "claude-opus-4-7"
    adapter_version: str | None = None


class RunManifest(BaseModel):
    run_id: str
    spec_hash: str
    prompt_hash: str
    agent: AgentInfo
    seed: int
    trial_id: int
    env_hash: str = Field(default_factory=_env_hash)
    created_at: str = Field(default_factory=_now_iso)
    bnf_version: str = "0.1.0"


class VendorUsage(BaseModel):
    source: str | None = None
    input_tokens: int = 0
    cache_creation_input_tokens: int = 0
    cache_read_input_tokens: int = 0
    output_tokens: int = 0
    reasoning_output_tokens: int = 0
    total_tokens: int = 0
    cost_usd: float | None = None


class UsageEvent(BaseModel):
    index: int
    source: str
    direction: Literal["model_to_harness", "harness_to_model", "metadata"]
    kind: Literal["assistant_text", "tool_call", "tool_result", "final_answer", "vendor_usage"]
    name: str | None = None
    event_id: str | None = None
    text_bytes: int = 0
    is_network: bool = False


class UsageMetrics(BaseModel):
    wall_clock_seconds: float = 0.0
    usage_source: str | None = None
    session_id: str | None = None
    trace_path: str | None = None
    prompt_bytes: int = 0
    retrieval_payload_bytes: int = 0
    tool_input_bytes: int = 0
    assistant_message_bytes: int = 0
    final_answer_bytes: int = 0
    bytes_to_model: int = 0
    bytes_from_model: int = 0
    tool_calls: int = 0
    network_requests: int = 0
    vendor_usage: VendorUsage | None = None
    #: Proxy-observed efficiency, populated when the runner supplied a
    #: :class:`bnf.proxy.Gateway`. The five axes below match paper §17.3
    #: exactly; everything here is the *novel* slice (deduped across
    #: replayed history).
    efficiency: "Efficiency | None" = None


class Efficiency(BaseModel):
    """Per-run efficiency measured by the Observation layer (proxy).

    Fields mirror the paper's §17.3 efficiency axes. Counts and bytes
    are reported at the novel / conversational level — see
    :mod:`bnf.proxy.parser` for dedup semantics.
    """

    # --- reported efficiency axes (paper §17.3) -----------------------
    #: Input side: UTF-8 bytes of tool_result content delivered to the
    #: model (deduped across replayed history).
    novel_retrieval_bytes: int = 0
    #: Output side: UTF-8 bytes of tool_call arguments emitted by the
    #: model (session-unique by construction; responses are never
    #: replayed). Argument phrasing is an agent-chosen efficiency
    #: behavior so it belongs in the cost accounting (preregistration).
    novel_tool_call_bytes: int = 0
    novel_tool_call_count: int = 0
    novel_tool_result_count: int = 0
    wall_clock_seconds: float = 0.0
    generation_tok_per_s: float | None = None

    # --- behavioural companion data ----------------------------------
    #: Frequency of tool names the model invoked (novel dedup).
    tool_names: dict[str, int] = Field(default_factory=dict)
    #: Number of turns detected (1 for single-prompt sessions).
    turn_count: int = 1

    # --- vendor usage, captured but not used for cross-vendor claims -
    #: Normalized vendor tokens (input/output split). Retained for
    #: within-family sensitivity analysis only; not part of the
    #: cross-vendor efficiency axes (paper §17.5).
    vendor_tokens: "VendorTokens | None" = None

    # --- diagnostics (for inspection, not headline reporting) --------
    proxy_session_id: str | None = None
    request_count: int = 0
    #: Raw wire bytes (includes everything — framing, overhead, replay).
    #: Use only for transport sanity, never for efficiency claims.
    raw_request_bytes: int = 0
    raw_response_bytes: int = 0


class VendorTokens(BaseModel):
    """Normalized token view across Anthropic and OpenAI.

    Semantics are cross-vendor consistent (paper §17.7 footnote): input
    tokens are split into ``cached`` / ``uncached`` using a single
    definition that both vendors can be rewritten into. See
    :class:`bnf.proxy.parser.UsageSnapshot` for the mapping.
    """

    total_input_tokens: int = 0
    cached_input_tokens: int = 0
    uncached_input_tokens: int = 0
    cache_creation_input_tokens: int = 0  # Anthropic-only
    non_reasoning_output_tokens: int = 0
    reasoning_output_tokens: int = 0
    total_tokens: int = 0


UsageMetrics.model_rebuild()


class ValidationResult(BaseModel):
    """Minimal build-artefact health check.

    The benchmark does not judge correctness here; we only record
    whether the project compiles and collect a few cheap structural
    signals for the run log. Find-phase accuracy is the real metric.
    """

    compile_pass: bool = False
    rust_files: int = 0
    rust_loc: int = 0
    details: dict[str, str] = Field(default_factory=dict)


class BuildRun(BaseModel):
    build_run_id: str
    task_id: str
    manifest: RunManifest
    artifact_path: str
    usage: UsageMetrics
    validation: ValidationResult
    status: Literal["ok", "error", "timeout"]
    error: str | None = None


class FinderAnswer(BaseModel):
    question_id: str
    presented_options: dict[str, str]  # after optional shuffle
    presented_correct_key: Literal["A", "B", "C", "D"]
    selected_option: Literal["A", "B", "C", "D"] | None
    is_correct: bool
    evidence_files: list[str] = Field(default_factory=list)
    evidence_symbols: list[str] = Field(default_factory=list)
    rationale: str | None = None


class FindRun(BaseModel):
    find_run_id: str
    build_run_id: str
    task_id: str
    manifest: RunManifest
    answers: list[FinderAnswer]
    usage: UsageMetrics
    status: Literal["ok", "error", "timeout"]
    error: str | None = None

    @property
    def accuracy(self) -> float:
        if not self.answers:
            return 0.0
        return sum(1 for a in self.answers if a.is_correct) / len(self.answers)


def write_run(obj: BuildRun | FindRun, dest_dir: str | Path) -> Path:
    """Atomically persist a run record.

    Writes to a sibling ``.tmp`` file first, then renames into place,
    so disk-full / crash mid-write can never leave a truncated JSON
    that a subsequent resume would see as a valid record.
    """

    import os
    import time

    d = Path(dest_dir)
    d.mkdir(parents=True, exist_ok=True)
    rid = obj.build_run_id if isinstance(obj, BuildRun) else obj.find_run_id
    path = d / f"{rid}.json"
    tmp = path.with_name(f"{path.name}.{os.getpid()}.{time.time_ns()}.tmp")
    tmp.write_text(obj.model_dump_json(indent=2), encoding="utf-8")
    os.replace(tmp, path)
    return path


def load_run_json(path: Path) -> dict | None:
    """Load a run JSON, returning ``None`` if the file is missing or
    cannot be parsed (e.g. legacy truncated-JSON leftovers)."""

    import json

    if not path.exists():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None
