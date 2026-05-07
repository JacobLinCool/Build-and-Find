"""Resume / idempotency tests.

A long benchmark batch can die mid-run (disk full, OOM, flaky vendor).
The runner is expected to pick up where it left off without rewriting
raw evidence: successful records are skipped, and failed, stale, or
incomplete records fail fast so the operator can resolve the collision
explicitly.

The test matrix covers:

* atomic ``write_run`` — disk-full mid-write doesn't leave a corrupted
  file visible to the next resume attempt;
* BuildRunner skips an existing ``status="ok"`` record with live
  artifact;
* BuildRunner refuses to overwrite when artifact was deleted;
* BuildRunner refuses to overwrite when previous status was ``error``;
* ``resume=False`` forces re-execution even on good records;
* FindRunner behaves the same for its own record type.

All tests use a stub adapter rather than BNF_DRY_RUN so the test can
observe exactly how many times ``run`` was invoked.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

import pytest

from bnf.build.runner import BuildRunner, _load_existing_build_run
from bnf.find.runner import FindRunner, _load_existing_find_run
from bnf.runners.base import AgentInvocation, AgentResult
from bnf.schema.run import (
    AgentInfo,
    BuildRun,
    FindRun,
    FinderAnswer,
    RunManifest,
    UsageMetrics,
    ValidationResult,
    write_run,
)


# ---------------------------------------------------------------------------
# Test doubles
# ---------------------------------------------------------------------------


@dataclass
class _StubAdapter:
    """Counts how many times ``run`` was called and returns a
    deterministic success."""

    id: str = "stub-agent"
    family: str = "claude_code"
    model_id: str = "stub-model"
    description: str = "stub"
    cli_bin: str = "/bin/true"
    calls: int = 0
    status_override: str = "ok"  # use "error" to simulate a prior failure

    def run(self, inv: AgentInvocation, *, gateway=None) -> AgentResult:  # noqa: ARG002
        self.calls += 1
        # Ensure the workspace has a minimal artifact so validators pass.
        inv.workspace.mkdir(parents=True, exist_ok=True)
        (inv.workspace / "Cargo.toml").write_text(
            '[package]\nname = "stub"\nversion = "0.0.0"\nedition = "2021"\n',
            encoding="utf-8",
        )
        src = inv.workspace / "src"
        src.mkdir(exist_ok=True)
        (src / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
        return AgentResult(
            status=self.status_override,
            stdout="",
            stderr="",
            usage=UsageMetrics(wall_clock_seconds=0.01),
            usage_events=[],
            error=None if self.status_override == "ok" else "synthetic failure",
        )


@dataclass
class _MinimalSpec:
    """Tiny shape-compatible spec substitute for the resume tests."""

    task_id: str = "stub-task"
    task_family: str = "build_from_scratch"

    @dataclass
    class _Constraints:
        build_time_budget_s: int = 30

    constraints: "_MinimalSpec._Constraints" = field(default_factory=_Constraints)

    def spec_hash(self) -> str:
        return "stub-spec-hash"


# ---------------------------------------------------------------------------
# Atomic write
# ---------------------------------------------------------------------------


def _make_build_run(run_id: str = "br-x", status: str = "ok") -> BuildRun:
    return BuildRun(
        build_run_id=run_id,
        task_id="t",
        manifest=RunManifest(
            run_id=run_id,
            spec_hash="h",
            prompt_hash="p",
            agent=AgentInfo(id="a", adapter="claude_code", model_id="m"),
            seed=0,
            trial_id=0,
        ),
        artifact_path="/tmp/irrelevant",
        usage=UsageMetrics(),
        validation=ValidationResult(compile_pass=True),
        status=status,
    )


def test_write_run_is_atomic(tmp_path, monkeypatch) -> None:
    """If ``write_text`` explodes mid-write (simulating a disk-full
    error), the target path must NOT exist — the temp file may, but
    the visible json is untouched."""

    target = tmp_path / "br-x.json"

    # Pre-seed a valid record so we can tell whether it was
    # overwritten by a botched atomic attempt.
    write_run(_make_build_run("br-x"), tmp_path)
    original = target.read_text(encoding="utf-8")

    # Monkeypatch write_text to blow up.
    real_write_text = Path.write_text

    def _boom(self, data, *args, **kwargs):
        if self.name.endswith(".tmp"):
            # Simulate the disk filling up partway through the tmp
            # write: let some bytes land, then raise.
            real_write_text(self, data[: len(data) // 2], *args, **kwargs)
            raise OSError(28, "No space left on device")
        return real_write_text(self, data, *args, **kwargs)

    monkeypatch.setattr(Path, "write_text", _boom)

    with pytest.raises(OSError):
        write_run(_make_build_run("br-x", status="error"), tmp_path)

    # Visible file is still the original — the half-written tmp lives
    # alongside (or gets cleaned up later) but never replaces it.
    assert target.read_text(encoding="utf-8") == original


# ---------------------------------------------------------------------------
# BuildRunner resume
# ---------------------------------------------------------------------------


def _prep_build_dirs(tmp_path: Path) -> dict[str, Path]:
    return {
        "specs_dir": tmp_path / "specs",
        "artifacts_dir": tmp_path / "artifacts",
        "runs_dir": tmp_path / "runs",
        "cargo_target_dir": tmp_path / "cargo",
    }


def _build_runner(tmp_path: Path, *, resume: bool = True) -> BuildRunner:
    dirs = _prep_build_dirs(tmp_path)
    for d in dirs.values():
        d.mkdir(parents=True, exist_ok=True)
    return BuildRunner(**dirs, resume=resume)


class _BuildRunnerNoSandbox(BuildRunner):
    """BuildRunner that skips ``prepare_workspace`` and
    ``validate_artifact`` so resume tests don't need a real Rust spec
    on disk. Keeps resume / run-detection semantics intact."""

    def run(self, spec, adapter, *, seed=0, trial_id=0):  # type: ignore[override]
        from bnf.build.runner import _load_existing_build_run, _mk_id, log

        build_run_id = _mk_id(spec.task_id, adapter.id, trial_id, seed)
        spec_hash = spec.spec_hash()
        prompt_hash = "stub-prompt-hash"
        if self.resume:
            existing = _load_existing_build_run(
                self.runs_dir,
                build_run_id,
                expected_spec_hash=spec_hash,
                expected_prompt_hash=prompt_hash,
            )
            if existing is not None:
                log.info("build skip %s (already ok)", build_run_id)
                return existing
        workspace = self.artifacts_dir / build_run_id
        workspace.mkdir(parents=True, exist_ok=True)
        inv = AgentInvocation(
            prompt="stub prompt",
            workspace=workspace,
            timeout_s=30,
        )
        result = adapter.run(inv, gateway=self.gateway)
        manifest = RunManifest(
            run_id=build_run_id,
            spec_hash=spec_hash,
            prompt_hash=prompt_hash,
            agent=AgentInfo(id=adapter.id, adapter=adapter.family, model_id=adapter.model_id),
            seed=seed,
            trial_id=trial_id,
        )
        build_run = BuildRun(
            build_run_id=build_run_id,
            task_id=spec.task_id,
            manifest=manifest,
            artifact_path=str(workspace.resolve()),
            usage=result.usage,
            validation=ValidationResult(compile_pass=True),
            status=result.status,
            error=result.error,
        )
        write_run(build_run, self.runs_dir)
        return build_run


def test_buildrunner_skips_existing_successful_record(tmp_path) -> None:
    dirs = _prep_build_dirs(tmp_path)
    for d in dirs.values():
        d.mkdir(parents=True, exist_ok=True)
    runner = _BuildRunnerNoSandbox(**dirs, resume=True)
    adapter = _StubAdapter()

    spec = _MinimalSpec(task_id="t-resume-ok")
    first = runner.run(spec, adapter)
    assert adapter.calls == 1
    assert first.status == "ok"

    # Second invocation must NOT re-run the adapter.
    second = runner.run(spec, adapter)
    assert adapter.calls == 1, "adapter must be skipped when record exists"
    assert second.build_run_id == first.build_run_id
    assert second.artifact_path == first.artifact_path


def test_buildrunner_fails_when_artifact_missing(tmp_path) -> None:
    dirs = _prep_build_dirs(tmp_path)
    for d in dirs.values():
        d.mkdir(parents=True, exist_ok=True)
    runner = _BuildRunnerNoSandbox(**dirs, resume=True)
    adapter = _StubAdapter()
    spec = _MinimalSpec(task_id="t-resume-no-artifact")
    first = runner.run(spec, adapter)
    # Nuke the artifact between runs to mimic an aggressive cleanup.
    import shutil as _sh

    _sh.rmtree(first.artifact_path)

    with pytest.raises(RuntimeError, match="artifact is missing"):
        runner.run(spec, adapter)
    assert adapter.calls == 1


def test_buildrunner_fails_after_prior_error(tmp_path) -> None:
    dirs = _prep_build_dirs(tmp_path)
    for d in dirs.values():
        d.mkdir(parents=True, exist_ok=True)
    runner = _BuildRunnerNoSandbox(**dirs, resume=True)
    adapter = _StubAdapter(status_override="error")
    spec = _MinimalSpec(task_id="t-resume-err")
    runner.run(spec, adapter)
    assert adapter.calls == 1

    # Next time, the record says status=error. It is raw evidence, so
    # resume must not overwrite it with a successful retry.
    adapter.status_override = "ok"
    with pytest.raises(RuntimeError, match="status='error'"):
        runner.run(spec, adapter)
    assert adapter.calls == 1


def test_buildrunner_force_mode_reruns_even_successful_records(tmp_path) -> None:
    dirs = _prep_build_dirs(tmp_path)
    for d in dirs.values():
        d.mkdir(parents=True, exist_ok=True)

    # First run with resume=True to seed the record.
    resuming = _BuildRunnerNoSandbox(**dirs, resume=True)
    adapter = _StubAdapter()
    spec = _MinimalSpec(task_id="t-force")
    resuming.run(spec, adapter)
    assert adapter.calls == 1

    # Second run with resume=False must re-execute.
    forcing = _BuildRunnerNoSandbox(**dirs, resume=False)
    forcing.run(spec, adapter)
    assert adapter.calls == 2


def test_buildrunner_corrupted_json_fails_fast(tmp_path) -> None:
    """A truncated / half-written JSON record (the disk-full scenario)
    must not be overwritten by resume; it needs explicit operator
    handling."""

    dirs = _prep_build_dirs(tmp_path)
    for d in dirs.values():
        d.mkdir(parents=True, exist_ok=True)
    spec = _MinimalSpec(task_id="t-corrupt")
    from bnf.build.runner import _mk_id

    adapter = _StubAdapter()
    run_id = _mk_id(spec.task_id, adapter.id, 0, 0)
    # Simulate a legacy truncated-JSON artifact.
    (dirs["runs_dir"] / f"{run_id}.json").write_text("{", encoding="utf-8")

    runner = _BuildRunnerNoSandbox(**dirs, resume=True)
    with pytest.raises(RuntimeError, match="not valid JSON"):
        runner.run(spec, adapter)
    assert adapter.calls == 0


# ---------------------------------------------------------------------------
# FindRunner resume
# ---------------------------------------------------------------------------


def _make_find_run(run_id: str, build_run_id: str, *, status: str = "ok") -> FindRun:
    return FindRun(
        find_run_id=run_id,
        build_run_id=build_run_id,
        task_id="t",
        manifest=RunManifest(
            run_id=run_id,
            spec_hash="h",
            prompt_hash="p",
            agent=AgentInfo(id="f", adapter="claude_code", model_id="m"),
            seed=0,
            trial_id=0,
        ),
        answers=[
            FinderAnswer(
                question_id="q1",
                presented_options={"A": "a", "B": "b", "C": "c", "D": "d"},
                presented_correct_key="A",
                selected_option="A",
                is_correct=True,
                evidence_files=[],
                evidence_symbols=[],
            )
        ],
        usage=UsageMetrics(),
        status=status,
    )


def test_find_resume_helper_returns_none_for_missing(tmp_path) -> None:
    assert (
        _load_existing_find_run(
            tmp_path, "does-not-exist", expected_spec_hash="h", expected_prompt_hash="p"
        )
        is None
    )


def test_find_resume_helper_fails_for_failed_record(tmp_path) -> None:
    fr = _make_find_run("fr-bad", "br-bad", status="error")
    write_run(fr, tmp_path)
    with pytest.raises(RuntimeError, match="status='error'"):
        _load_existing_find_run(
            tmp_path, "fr-bad", expected_spec_hash="h", expected_prompt_hash="p"
        )


def test_find_resume_helper_loads_successful_record(tmp_path) -> None:
    fr = _make_find_run("fr-good", "br-good")
    write_run(fr, tmp_path)
    loaded = _load_existing_find_run(
        tmp_path, "fr-good", expected_spec_hash="h", expected_prompt_hash="p"
    )
    assert loaded is not None
    assert loaded.find_run_id == "fr-good"
    assert loaded.status == "ok"


def test_find_resume_helper_invalidates_on_spec_hash_change(tmp_path) -> None:
    fr = _make_find_run("fr-good", "br-good")
    write_run(fr, tmp_path)
    # Same prompt, different spec → record is stale (spec was edited)
    # and must not be overwritten.
    with pytest.raises(RuntimeError, match="spec_hash differs"):
        _load_existing_find_run(
            tmp_path, "fr-good", expected_spec_hash="h-new", expected_prompt_hash="p"
        )


def test_find_resume_helper_invalidates_on_prompt_hash_change(tmp_path) -> None:
    fr = _make_find_run("fr-good", "br-good")
    write_run(fr, tmp_path)
    # Question YAML edited → prompt_hash diverged, so resume must
    # fail rather than overwrite the old run.
    with pytest.raises(RuntimeError, match="prompt_hash differs"):
        _load_existing_find_run(
            tmp_path, "fr-good", expected_spec_hash="h", expected_prompt_hash="p-new"
        )


def test_build_resume_helper_fails_when_artifact_missing(tmp_path) -> None:
    br = _make_build_run("br-nope")
    # artifact_path points at a nonexistent dir.
    br.artifact_path = str(tmp_path / "nope-not-here")
    write_run(br, tmp_path)
    with pytest.raises(RuntimeError, match="artifact is missing"):
        _load_existing_build_run(
            tmp_path, "br-nope", expected_spec_hash="h", expected_prompt_hash="p"
        )


def test_build_resume_helper_accepts_live_artifact(tmp_path) -> None:
    artifact = tmp_path / "art"
    artifact.mkdir()
    br = _make_build_run("br-live")
    br.artifact_path = str(artifact)
    write_run(br, tmp_path)
    loaded = _load_existing_build_run(
        tmp_path, "br-live", expected_spec_hash="h", expected_prompt_hash="p"
    )
    assert loaded is not None
    assert loaded.build_run_id == "br-live"


def test_build_resume_helper_invalidates_on_spec_hash_change(tmp_path) -> None:
    artifact = tmp_path / "art"
    artifact.mkdir()
    br = _make_build_run("br-live")
    br.artifact_path = str(artifact)
    write_run(br, tmp_path)
    # Spec edited → builder prompt would change → record is stale.
    with pytest.raises(RuntimeError, match="spec_hash differs"):
        _load_existing_build_run(
            tmp_path, "br-live", expected_spec_hash="h-new", expected_prompt_hash="p"
        )


def test_build_resume_helper_invalidates_on_prompt_hash_change(tmp_path) -> None:
    artifact = tmp_path / "art"
    artifact.mkdir()
    br = _make_build_run("br-live")
    br.artifact_path = str(artifact)
    write_run(br, tmp_path)
    with pytest.raises(RuntimeError, match="prompt_hash differs"):
        _load_existing_build_run(
            tmp_path, "br-live", expected_spec_hash="h", expected_prompt_hash="p-new"
        )


# ---------------------------------------------------------------------------
# FindRunner refuses to score against a failed build
# ---------------------------------------------------------------------------


def test_find_runner_refuses_failed_build(tmp_path) -> None:
    """An ``status='error'`` build leaves a half-written artifact on disk
    (the agent crashed mid-write or returned non-zero). FindRunner must
    refuse to score against it — otherwise finders answer questions
    about an unfinished codebase and the matrix shows noise that looks
    like real builder/finder behavior.
    """

    # Make an artifact dir that exists (the broken build did write some
    # files) and a BuildRun record marking the run as errored.
    artifact = tmp_path / "broken-art"
    artifact.mkdir()
    br = _make_build_run("br-broken", status="error")
    br.artifact_path = str(artifact)

    # Minimal stand-ins; we expect to short-circuit before they're used.
    runner = FindRunner(
        runs_dir=tmp_path / "runs",
        find_workspaces_dir=tmp_path / "find_ws",
        cargo_target_dir=tmp_path / "cargo",
    )
    runner.runs_dir.mkdir()

    spec = _MinimalSpec(task_id="t-broken")

    import pytest as _pytest

    with _pytest.raises(ValueError, match="status='error'"):
        runner.run(spec, bank=None, build_run=br, adapter=_StubAdapter())  # type: ignore[arg-type]
