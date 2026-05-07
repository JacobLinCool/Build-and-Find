"""Orchestrates a single Find run.

Steps:

1. Clone the codebase at ``build_run.artifact_path`` into an isolated
   finder workspace so the finder can write files without mutating the
   build artefact.
2. Render the finder prompt with seeded option shuffling.
3. Invoke the finder adapter inside that workspace.
4. Read the finder's answers from ``find_answers.json`` in the workspace.
5. Re-key each selection against the *presented* correct option and score.
6. Persist a ``FindRun`` JSON record.
"""

from __future__ import annotations

import hashlib
import logging
import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

from bnf.prompts import (
    ANSWERS_FILENAME,
    finder_prompt_hash,
    read_finder_answers,
    render_finder_prompt,
)
from bnf.runners import AgentAdapter, AgentInvocation
from bnf.runners.base import detect_infrastructure_failure
from bnf.runners.usage_trace import write_usage_events
from bnf.schema import BuildTaskSpec, QuestionBank
from bnf.schema.run import (
    AgentInfo,
    BuildRun,
    FindRun,
    FinderAnswer,
    RunManifest,
    load_run_json,
    write_run,
)

if TYPE_CHECKING:
    from bnf.proxy import Gateway

log = logging.getLogger(__name__)


@dataclass
class FindRunner:
    runs_dir: Path
    find_workspaces_dir: Path
    cargo_target_dir: Path
    # Whether to randomise option order. Strongly recommended; disable only
    # for reproducing an older experiment.
    shuffle_options: bool = True
    #: Optional proxy gateway. When set, each Find invocation's vendor
    #: traffic is routed through it and the resulting
    #: :class:`~bnf.schema.run.Efficiency` is attached to the FindRun.
    gateway: "Gateway | None" = None
    #: Skip runs that already have a successful on-disk record.
    #: Required for interrupted-batch recovery. If a record with the
    #: same run id exists but is failed, stale, or corrupt, resume fails
    #: fast rather than overwriting raw evidence.
    resume: bool = True

    def run(
        self,
        spec: BuildTaskSpec,
        bank: QuestionBank,
        build_run: BuildRun,
        adapter: AgentAdapter,
        *,
        seed: int = 0,
        trial_id: int = 0,
    ) -> FindRun:
        src = Path(build_run.artifact_path)
        if not src.exists():
            raise FileNotFoundError(f"codebase missing at {src}")
        if build_run.status != "ok":
            # Refuse to score a finder against a half-finished codebase.
            # The artifact directory may exist (the agent wrote some
            # files before erroring) but is not the deliverable any
            # spec contemplates. Caller should retry the build first.
            raise ValueError(
                f"refusing to find against build {build_run.build_run_id} "
                f"with status={build_run.status!r}; rebuild it first "
                f"(e.g. `bnf build run --task {build_run.task_id} "
                f"--builder {build_run.manifest.agent.id} --force`)"
            )

        find_run_id = _mk_id(build_run.build_run_id, adapter.id, trial_id, seed)

        # Render prompt up front so resume can refuse stale records whose
        # spec or shuffled-prompt hash no longer matches the current
        # bank / shuffle seed. Without this, edits to a question YAML
        # would silently keep yielding the old answers via --resume.
        spec_hash = spec.spec_hash()
        shuffle_seed = seed if self.shuffle_options else None
        prompt, presentation = render_finder_prompt(bank, shuffle_seed=shuffle_seed)
        prompt_hash = finder_prompt_hash(prompt)

        existing = (
            _load_existing_find_run(
                self.runs_dir,
                find_run_id,
                expected_spec_hash=spec_hash,
                expected_prompt_hash=prompt_hash,
            )
            if self.resume
            else None
        )
        if existing is not None:
            log.info("find skip %s (already ok, resume=%s)", find_run_id, self.resume)
            return existing
        workspace = self.find_workspaces_dir / find_run_id
        _clone_codebase(src, workspace)

        self.cargo_target_dir.mkdir(parents=True, exist_ok=True)
        inv = AgentInvocation(
            prompt=prompt,
            workspace=workspace,
            timeout_s=spec.constraints.build_time_budget_s,
            env={"CARGO_TARGET_DIR": str(self.cargo_target_dir.resolve())},
        )
        log.info("find start %s (%s on %s)", find_run_id, adapter.id, build_run.build_run_id)
        result = adapter.run(inv, gateway=self.gateway)
        log.info("find end %s status=%s", find_run_id, result.status)

        parsed = read_finder_answers(workspace)
        answers: list[FinderAnswer] = []
        for q in bank.questions:
            presented_opts, presented_correct = presentation[q.question_id]
            pick = parsed.get(q.question_id) or {}
            sel = pick.get("selected_option")
            is_correct = sel == presented_correct
            answers.append(
                FinderAnswer(
                    question_id=q.question_id,
                    presented_options=presented_opts,
                    presented_correct_key=presented_correct,
                    selected_option=sel,
                    is_correct=is_correct,
                    evidence_files=pick.get("evidence_files") or [],
                    evidence_symbols=pick.get("evidence_symbols") or [],
                    rationale=pick.get("rationale"),
                )
            )

        manifest = RunManifest(
            run_id=find_run_id,
            spec_hash=spec.spec_hash(),
            prompt_hash=prompt_hash,
            agent=AgentInfo(
                id=adapter.id,
                adapter=adapter.family,  # type: ignore[arg-type]
                model_id=adapter.model_id,
            ),
            seed=seed,
            trial_id=trial_id,
        )
        find_run = FindRun(
            find_run_id=find_run_id,
            build_run_id=build_run.build_run_id,
            task_id=spec.task_id,
            manifest=manifest,
            answers=answers,
            usage=result.usage,
            status=result.status,  # type: ignore[arg-type]
            error=result.error,
        )
        write_run(find_run, self.runs_dir)
        forensics = self.runs_dir / f"{find_run_id}.logs"
        forensics.mkdir(parents=True, exist_ok=True)
        (forensics / "prompt.txt").write_text(prompt, encoding="utf-8")
        (forensics / "stdout.txt").write_text(result.stdout, encoding="utf-8")
        (forensics / "stderr.txt").write_text(result.stderr, encoding="utf-8")
        write_usage_events(forensics / "usage_events.jsonl", result.usage_events)
        # Copy the answers file into forensics for auditing.
        answers_src = workspace / ANSWERS_FILENAME
        if answers_src.exists():
            shutil.copy2(answers_src, forensics / ANSWERS_FILENAME)
        infra_reason = detect_infrastructure_failure(result)
        if infra_reason is not None:
            raise RuntimeError(
                f"infrastructure failure in {find_run_id}: {infra_reason}; "
                f"wrote raw evidence to {self.runs_dir / f'{find_run_id}.json'} "
                f"and logs to {forensics}"
            )
        return find_run


_CLONE_IGNORES = ("target", ".git", "node_modules", ".venv", "__pycache__")


def _clone_codebase(src: Path, dst: Path) -> None:
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(
        src,
        dst,
        dirs_exist_ok=False,
        ignore=shutil.ignore_patterns(*_CLONE_IGNORES),
    )


def _load_existing_find_run(
    runs_dir: Path,
    find_run_id: str,
    *,
    expected_spec_hash: str,
    expected_prompt_hash: str,
) -> "FindRun | None":
    """Return the on-disk :class:`FindRun` for this id iff it finished
    successfully and its recorded ``spec_hash`` / ``prompt_hash`` still
    match the current inputs. Errors, timeouts, corrupt records, and
    stale hashes are raw evidence, so resume refuses to overwrite them.

    The hash comparison is what makes ``--resume`` safe across question
    bank edits: changing a question rewrites ``prompt_hash`` and
    invalidates every cached find run on the affected task.
    """

    path = runs_dir / f"{find_run_id}.json"
    payload = load_run_json(path)
    if payload is None:
        if path.exists():
            raise RuntimeError(f"find record exists but is not valid JSON: {path}")
        return None
    fr = FindRun.model_validate(payload)
    if fr.status != "ok":
        raise RuntimeError(
            f"find record {find_run_id} has status={fr.status!r}; refusing to overwrite "
            f"raw evidence at {path}"
        )
    if fr.manifest.spec_hash != expected_spec_hash:
        raise RuntimeError(
            f"find record {find_run_id} spec_hash differs from current spec; refusing "
            f"to overwrite raw evidence at {path}"
        )
    if fr.manifest.prompt_hash != expected_prompt_hash:
        raise RuntimeError(
            f"find record {find_run_id} prompt_hash differs from current prompt; refusing "
            f"to overwrite raw evidence at {path}"
        )
    return fr


def _mk_id(build_run_id: str, finder_id: str, trial_id: int, seed: int) -> str:
    payload = f"{build_run_id}|{finder_id}|{trial_id}|{seed}".encode()
    short = hashlib.sha256(payload).hexdigest()[:8]
    return f"find-{build_run_id[:24]}-{finder_id}-t{trial_id}-s{seed}-{short}"
