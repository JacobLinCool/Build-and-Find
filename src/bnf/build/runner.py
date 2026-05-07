"""Orchestrates a single Build run.

A Build run is uniquely identified by ``(task_id, builder_id, trial_id, seed)``.
The runner:

1. Prepares an isolated workspace (copies seed for edit tasks).
2. Renders the builder prompt and computes its hash.
3. Invokes the agent adapter with ``CARGO_TARGET_DIR`` pointing at a
   shared scratch directory so the build artefact stays source-only.
4. Captures usage metrics.
5. Runs validation (compile probe) using the same shared target dir.
6. Persists a ``BuildRun`` JSON record alongside the artifact.
"""

from __future__ import annotations

import hashlib
import logging
import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

from bnf.build.sandbox import prepare_workspace
from bnf.build.validator import validate_artifact
from bnf.prompts import builder_prompt_hash, render_builder_prompt
from bnf.runners import AgentAdapter, AgentInvocation
from bnf.runners.base import detect_infrastructure_failure
from bnf.runners.usage_trace import write_usage_events
from bnf.schema import BuildTaskSpec
from bnf.schema.run import AgentInfo, BuildRun, RunManifest, load_run_json, write_run

if TYPE_CHECKING:
    from bnf.proxy import Gateway

log = logging.getLogger(__name__)


@dataclass
class BuildRunner:
    specs_dir: Path
    artifacts_dir: Path
    runs_dir: Path
    cargo_target_dir: Path
    #: Optional proxy gateway. When set, all adapter runs route their
    #: vendor traffic through it and the resulting
    #: :class:`~bnf.schema.run.Efficiency` is attached to the BuildRun.
    gateway: "Gateway | None" = None
    #: Skip runs that already have a successful on-disk record.
    #: Required for interrupted-batch recovery. If a record with the
    #: same run id exists but is failed, stale, corrupt, or missing its
    #: artifact, resume fails fast rather than overwriting raw evidence.
    resume: bool = True

    def run(
        self,
        spec: BuildTaskSpec,
        adapter: AgentAdapter,
        *,
        seed: int = 0,
        trial_id: int = 0,
    ) -> BuildRun:
        build_run_id = _mk_id(spec.task_id, adapter.id, trial_id, seed)

        # Compute current hashes up front so resume can refuse stale records
        # whose spec/prompt no longer matches the on-disk inputs.
        spec_hash = spec.spec_hash()
        prompt = render_builder_prompt(spec)
        prompt_hash = builder_prompt_hash(prompt)

        existing = (
            _load_existing_build_run(
                self.runs_dir,
                build_run_id,
                expected_spec_hash=spec_hash,
                expected_prompt_hash=prompt_hash,
            )
            if self.resume
            else None
        )
        if existing is not None:
            log.info("build skip %s (already ok, resume=%s)", build_run_id, self.resume)
            return existing
        workspace = self.artifacts_dir / build_run_id
        prepare_workspace(spec, workspace, specs_dir=self.specs_dir)

        self.cargo_target_dir.mkdir(parents=True, exist_ok=True)
        inv = AgentInvocation(
            prompt=prompt,
            workspace=workspace,
            timeout_s=spec.constraints.build_time_budget_s,
            env={"CARGO_TARGET_DIR": str(self.cargo_target_dir.resolve())},
        )
        log.info("build start %s (%s -> %s)", build_run_id, spec.task_id, adapter.id)
        result = adapter.run(inv, gateway=self.gateway)
        log.info("build end %s status=%s", build_run_id, result.status)

        # Safety net: if the agent wrote a target/ inside the artefact
        # anyway (ignoring our CARGO_TARGET_DIR), nuke it now before any
        # downstream clone amplifies the disk cost.
        inner_target = workspace / "target"
        if inner_target.exists():
            shutil.rmtree(inner_target, ignore_errors=True)

        validation = validate_artifact(
            spec,
            workspace,
            specs_dir=self.specs_dir,
            cargo_target_dir=self.cargo_target_dir,
        )

        manifest = RunManifest(
            run_id=build_run_id,
            spec_hash=spec_hash,
            prompt_hash=prompt_hash,
            agent=AgentInfo(
                id=adapter.id,
                adapter=adapter.family,  # type: ignore[arg-type]
                model_id=adapter.model_id,
            ),
            seed=seed,
            trial_id=trial_id,
        )
        build_run = BuildRun(
            build_run_id=build_run_id,
            task_id=spec.task_id,
            manifest=manifest,
            artifact_path=str(workspace.resolve()),
            usage=result.usage,
            validation=validation,
            status=result.status,  # type: ignore[arg-type]
            error=result.error,
        )
        write_run(build_run, self.runs_dir)
        forensics = workspace.parent / f"{build_run_id}.logs"
        forensics.mkdir(parents=True, exist_ok=True)
        (forensics / "prompt.txt").write_text(prompt, encoding="utf-8")
        (forensics / "stdout.txt").write_text(result.stdout, encoding="utf-8")
        (forensics / "stderr.txt").write_text(result.stderr, encoding="utf-8")
        write_usage_events(forensics / "usage_events.jsonl", result.usage_events)
        infra_reason = detect_infrastructure_failure(result)
        if infra_reason is not None:
            raise RuntimeError(
                f"infrastructure failure in {build_run_id}: {infra_reason}; "
                f"wrote raw evidence to {self.runs_dir / f'{build_run_id}.json'} "
                f"and logs to {forensics}"
            )
        return build_run


def _load_existing_build_run(
    runs_dir: Path,
    build_run_id: str,
    *,
    expected_spec_hash: str,
    expected_prompt_hash: str,
) -> "BuildRun | None":
    """Return the on-disk :class:`BuildRun` for this id iff it finished
    successfully, its artifact is still present, and its recorded
    ``spec_hash`` / ``prompt_hash`` still match the current inputs.

    A run that ended with ``status != "ok"`` or whose artifact was
    deleted is not reusable, but it is still raw evidence. Re-running
    the same id would overwrite that evidence, so callers must resolve
    the collision explicitly before continuing.
    """

    path = runs_dir / f"{build_run_id}.json"
    payload = load_run_json(path)
    if payload is None:
        if path.exists():
            raise RuntimeError(f"build record exists but is not valid JSON: {path}")
        return None
    br = BuildRun.model_validate(payload)
    if br.status != "ok":
        raise RuntimeError(
            f"build record {build_run_id} has status={br.status!r}; refusing to overwrite "
            f"raw evidence at {path}"
        )
    if not Path(br.artifact_path).exists():
        raise RuntimeError(
            f"build record {build_run_id} artifact is missing; refusing to overwrite "
            f"raw evidence at {path}"
        )
    if br.manifest.spec_hash != expected_spec_hash:
        raise RuntimeError(
            f"build record {build_run_id} spec_hash differs from current spec; refusing "
            f"to overwrite raw evidence at {path}"
        )
    if br.manifest.prompt_hash != expected_prompt_hash:
        raise RuntimeError(
            f"build record {build_run_id} prompt_hash differs from current prompt; refusing "
            f"to overwrite raw evidence at {path}"
        )
    return br


def _mk_id(task_id: str, builder_id: str, trial_id: int, seed: int) -> str:
    payload = f"{task_id}|{builder_id}|{trial_id}|{seed}".encode()
    short = hashlib.sha256(payload).hexdigest()[:8]
    return f"build-{task_id}-{builder_id}-t{trial_id}-s{seed}-{short}"
