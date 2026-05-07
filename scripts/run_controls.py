"""Run prior-knowledge and specification-leakage controls for Build-and-Find."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
from pathlib import Path

from bnf.config import paths
from bnf.orchestration import WorkItem, WorkResult, run_work_queue
from bnf.prompts import ANSWERS_FILENAME, finder_prompt_hash, read_finder_answers
from bnf.proxy import Gateway
from bnf.runners import AgentInvocation, AgentResult, get_agent, list_agents
from bnf.runners.base import detect_infrastructure_failure
from bnf.runners.usage_trace import write_usage_events
from bnf.schema import BuildTaskSpec, QuestionBank, load_question_bank, load_specs_dir
from bnf.schema.run import (
    AgentInfo,
    BuildRun,
    FinderAnswer,
    FindRun,
    RunManifest,
    UsageMetrics,
    ValidationResult,
    load_run_json,
    write_run,
)

CONTROL_CONDITIONS = ("spec_only", "question_only")


def main() -> None:
    args = _parse_args()
    p = paths()
    specs = _selected_specs(p.specs_dir, args.tasks)
    finder_ids = _split_csv(args.finders) or [agent.id for agent in list_agents()]
    conditions = tuple(_split_csv(args.conditions) or CONTROL_CONDITIONS)
    unknown_conditions = sorted(set(conditions) - set(CONTROL_CONDITIONS))
    if unknown_conditions:
        raise SystemExit(f"unknown control condition(s): {', '.join(unknown_conditions)}")

    gateway = None if args.no_proxy else Gateway.start()
    if gateway is not None:
        print(f"proxy listening on {gateway.base_url}")

    try:
        for spec in specs:
            bank = load_question_bank(p.questions_dir / f"{spec.task_id}.yaml")
            if "spec_only" in conditions:
                build = _ensure_control_build(p, spec, "spec_only", spec_text=_spec_text(p, spec))
                _run_control_finds(
                    p,
                    spec,
                    bank,
                    build,
                    finder_ids,
                    args.trials,
                    args.seed,
                    args.recovery_seed,
                    args.workers,
                    gateway,
                )
            if "question_only" in conditions:
                build = _ensure_control_build(p, spec, "question_only")
                _run_control_finds(
                    p,
                    spec,
                    bank,
                    build,
                    finder_ids,
                    args.trials,
                    args.seed,
                    args.recovery_seed,
                    args.workers,
                    gateway,
                )
    finally:
        if gateway is not None:
            gateway.stop()


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tasks", default="scratch_minidb,scratch_nanoweb")
    parser.add_argument("--finders", default="")
    parser.add_argument("--conditions", default=",".join(CONTROL_CONDITIONS))
    parser.add_argument("--trials", type=int, default=1)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--recovery-seed", type=int, default=20261501)
    parser.add_argument("--workers", type=int, default=1)
    parser.add_argument("--no-proxy", action="store_true")
    return parser.parse_args()


def _split_csv(value: str) -> list[str]:
    return [part.strip() for part in value.split(",") if part.strip()]


def _selected_specs(specs_dir: Path, tasks: str) -> list[BuildTaskSpec]:
    wanted = set(_split_csv(tasks))
    specs = [spec for spec in load_specs_dir(specs_dir) if spec.task_id in wanted]
    missing = wanted - {spec.task_id for spec in specs}
    if missing:
        raise SystemExit(f"unknown task id(s): {', '.join(sorted(missing))}")
    return specs


def _spec_text(p, spec: BuildTaskSpec) -> str:
    return (p.specs_dir / f"{spec.task_id}.yaml").read_text(encoding="utf-8")


def _ensure_control_build(
    p,
    spec: BuildTaskSpec,
    condition: str,
    *,
    spec_text: str | None = None,
) -> BuildRun:
    build_run_id = _control_build_id(spec.task_id, condition)
    artifact = p.artifacts_dir / build_run_id
    record_path = p.runs_dir / f"{build_run_id}.json"
    existing = load_run_json(record_path)
    if existing is not None:
        if not artifact.exists():
            raise RuntimeError(f"control build record exists without artifact: {record_path}")
        return BuildRun.model_validate(existing)

    artifact.mkdir(parents=True, exist_ok=True)
    (artifact / "CONTROL.md").write_text(
        f"condition: {condition}\ntarget_task_id: {spec.task_id}\n",
        encoding="utf-8",
    )
    if spec_text is not None:
        (artifact / "SPEC.md").write_text(spec_text, encoding="utf-8")

    prompt_hash = _control_prompt_hash(condition, spec.task_id)
    build = BuildRun(
        build_run_id=build_run_id,
        task_id=spec.task_id,
        manifest=RunManifest(
            run_id=build_run_id,
            spec_hash=spec.spec_hash(),
            prompt_hash=prompt_hash,
            agent=AgentInfo(
                id=f"control_{condition}",
                adapter="codex",
                model_id=condition,
            ),
            seed=0,
            trial_id=0,
        ),
        artifact_path=str(artifact.resolve()),
        usage=UsageMetrics(),
        validation=ValidationResult(
            compile_pass=False,
            details={
                "control_condition": condition,
            },
        ),
        status="ok",
    )
    write_run(build, p.runs_dir)
    return build


def _run_control_finds(
    p,
    spec: BuildTaskSpec,
    bank: QuestionBank,
    build: BuildRun,
    finder_ids: list[str],
    trials: int,
    seed: int,
    recovery_seed: int,
    workers: int,
    gateway: Gateway | None,
) -> None:
    condition = build.manifest.agent.id.removeprefix("control_")
    items: list[WorkItem[FindRun]] = []
    for finder_id in finder_ids:
        for trial_id in range(trials):
            run_seed = _select_control_find_seed(
                p,
                spec,
                bank,
                build,
                finder_id,
                trial_id,
                planned_seed=seed + trial_id,
                recovery_seed=recovery_seed + trial_id,
            )
            if run_seed is None:
                continue
            items.append(
                WorkItem(
                    name=f"control {condition} {spec.task_id} {finder_id} t{trial_id}",
                    run=lambda finder_id=finder_id,
                    trial_id=trial_id,
                    run_seed=run_seed: _run_one_control_find(
                        p,
                        spec,
                        bank,
                        build,
                        condition,
                        finder_id,
                        trial_id,
                        run_seed,
                        gateway,
                    ),
                )
            )
    run_work_queue(items, workers=workers, on_result=_print_control_progress)


def _run_one_control_find(
    p,
    spec: BuildTaskSpec,
    bank: QuestionBank,
    build: BuildRun,
    condition: str,
    finder_id: str,
    trial_id: int,
    run_seed: int,
    gateway: Gateway | None,
) -> FindRun:
    adapter = get_agent(finder_id)
    prompt, presentation = _render_control_prompt(
        condition,
        spec,
        bank,
        artifact_path=Path(build.artifact_path),
        shuffle_seed=run_seed,
    )
    find_run_id = _control_find_id(build.build_run_id, finder_id, trial_id, run_seed)
    existing = _existing_find(p.runs_dir, find_run_id, spec.spec_hash(), finder_prompt_hash(prompt))
    if existing is not None:
        return existing
    workspace = p.find_workspaces_dir / find_run_id
    if workspace.exists():
        shutil.rmtree(workspace)
    shutil.copytree(
        build.artifact_path,
        workspace,
        ignore=shutil.ignore_patterns("target", ".git", "node_modules", ".venv", "__pycache__"),
    )
    result = adapter.run(
        AgentInvocation(
            prompt=prompt,
            workspace=workspace,
            timeout_s=spec.constraints.build_time_budget_s,
            env={"CARGO_TARGET_DIR": str(p.cargo_target_dir.resolve())},
        ),
        gateway=gateway,
    )
    parsed = read_finder_answers(workspace)
    answers = []
    for question in bank.questions:
        presented_options, presented_correct = presentation[question.question_id]
        pick = parsed.get(question.question_id) or {}
        selected = pick.get("selected_option")
        answers.append(
            FinderAnswer(
                question_id=question.question_id,
                presented_options=presented_options,
                presented_correct_key=presented_correct,
                selected_option=selected,
                is_correct=selected == presented_correct,
                evidence_files=pick.get("evidence_files") or [],
                evidence_symbols=pick.get("evidence_symbols") or [],
                rationale=pick.get("rationale"),
            )
        )
    run = FindRun(
        find_run_id=find_run_id,
        build_run_id=build.build_run_id,
        task_id=spec.task_id,
        manifest=RunManifest(
            run_id=find_run_id,
            spec_hash=spec.spec_hash(),
            prompt_hash=finder_prompt_hash(prompt),
            agent=AgentInfo(
                id=adapter.id,
                adapter=adapter.family,  # type: ignore[arg-type]
                model_id=adapter.model_id,
            ),
            seed=run_seed,
            trial_id=trial_id,
        ),
        answers=answers,
        usage=result.usage,
        status=result.status,  # type: ignore[arg-type]
        error=result.error,
    )
    write_run(run, p.runs_dir)
    logs = p.runs_dir / f"{find_run_id}.logs"
    logs.mkdir(parents=True, exist_ok=True)
    (logs / "prompt.txt").write_text(prompt, encoding="utf-8")
    (logs / "stdout.txt").write_text(result.stdout, encoding="utf-8")
    (logs / "stderr.txt").write_text(result.stderr, encoding="utf-8")
    write_usage_events(logs / "usage_events.jsonl", result.usage_events)
    answers_src = workspace / ANSWERS_FILENAME
    if answers_src.exists():
        shutil.copy2(answers_src, logs / ANSWERS_FILENAME)
    infra_reason = detect_infrastructure_failure(result)
    if infra_reason is not None:
        raise RuntimeError(
            f"infrastructure failure in {find_run_id}: {infra_reason}; "
            f"wrote raw evidence to {p.runs_dir / f'{find_run_id}.json'} "
            f"and logs to {logs}"
        )
    return run


def _print_control_progress(result: WorkResult[FindRun], completed: int, total: int) -> None:
    print(
        f"control {completed}/{total} {result.value.find_run_id} "
        f"status={result.value.status} acc={result.value.accuracy:.2f} "
        f"elapsed={result.elapsed_seconds:.1f}s"
    )


def _select_control_find_seed(
    p,
    spec: BuildTaskSpec,
    bank: QuestionBank,
    build: BuildRun,
    finder_id: str,
    trial_id: int,
    *,
    planned_seed: int,
    recovery_seed: int,
) -> int | None:
    condition = build.manifest.agent.id.removeprefix("control_")

    def prompt_hash_for(seed: int) -> str:
        prompt, _ = _render_control_prompt(
            condition,
            spec,
            bank,
            artifact_path=Path(build.artifact_path),
            shuffle_seed=seed,
        )
        return finder_prompt_hash(prompt)

    planned_id = _control_find_id(build.build_run_id, finder_id, trial_id, planned_seed)
    planned = _load_control_find(p.runs_dir, planned_id)
    if planned is None:
        return planned_seed
    if planned.status == "ok":
        _require_matching_control_find(planned, spec.spec_hash(), prompt_hash_for(planned_seed))
        print(f"skip {planned_id}")
        return None
    _require_infrastructure_failure(p.runs_dir, planned_id, planned.status, planned.error)

    for attempt in range(100):
        seed = recovery_seed + attempt * 1000
        run_id = _control_find_id(build.build_run_id, finder_id, trial_id, seed)
        existing = _load_control_find(p.runs_dir, run_id)
        if existing is None:
            return seed
        if existing.status == "ok":
            _require_matching_control_find(existing, spec.spec_hash(), prompt_hash_for(seed))
            print(f"skip {run_id}")
            return None
        _require_infrastructure_failure(p.runs_dir, run_id, existing.status, existing.error)
    raise RuntimeError(f"no available recovery seed for {planned_id}")


def _render_control_prompt(
    condition: str,
    spec: BuildTaskSpec,
    bank: QuestionBank,
    *,
    artifact_path: Path,
    shuffle_seed: int,
) -> tuple[str, dict[str, tuple[dict[str, str], str]]]:
    if condition == "spec_only":
        context = (
            "The workspace contains the task specification in "
            "`SPEC.md`, but no implementation. Answer only what can be inferred from the "
            "specification; do not assume implementation details that are not stated."
        )
    elif condition == "question_only":
        context = (
            "The workspace contains no implementation and no "
            "task specification. Answer only from the question text and your prior knowledge."
        )
    else:
        raise ValueError(condition)

    presentation: dict[str, tuple[dict[str, str], str]] = {}
    parts = [
        context,
        "",
        f"Control condition: {condition}",
        f"Target task: {spec.task_id}",
        f"Workspace: {artifact_path.name}",
        "",
        "Write answers to `find_answers.json` as:",
        '{"answers":[{"question_id":"...","selected_option":"A","evidence_files":[],"evidence_symbols":[],"rationale":"..."}]}',
        "",
        f"Total questions: {len(bank.questions)}",
        "",
    ]
    for question in bank.questions:
        options, correct = question.shuffled(shuffle_seed)
        presentation[question.question_id] = (options, correct)
        parts.append(f"Question {question.question_id}")
        parts.append(question.prompt.strip())
        parts.append("")
        for key in ("A", "B", "C", "D"):
            parts.append(f"  {key}. {options[key]}")
        parts.append("")
    return "\n".join(parts).rstrip() + "\n", presentation


def _existing_find(
    runs_dir: Path,
    run_id: str,
    expected_spec_hash: str,
    expected_prompt_hash: str,
) -> FindRun | None:
    path = runs_dir / f"{run_id}.json"
    payload = load_run_json(path)
    if payload is None:
        if path.exists():
            raise RuntimeError(f"control find record exists but is not valid JSON: {path}")
        return None
    run = FindRun.model_validate(payload)
    if run.status != "ok":
        raise RuntimeError(
            f"control find record {run_id} has status={run.status!r}; refusing to overwrite "
            f"raw evidence at {path}"
        )
    if run.manifest.spec_hash != expected_spec_hash:
        raise RuntimeError(
            f"control find record {run_id} spec_hash differs from current spec; refusing "
            f"to overwrite raw evidence at {path}"
        )
    if run.manifest.prompt_hash != expected_prompt_hash:
        raise RuntimeError(
            f"control find record {run_id} prompt_hash differs from current prompt; refusing "
            f"to overwrite raw evidence at {path}"
        )
    return run


def _load_control_find(runs_dir: Path, run_id: str) -> FindRun | None:
    path = runs_dir / f"{run_id}.json"
    payload = load_run_json(path)
    if payload is None:
        if path.exists():
            raise RuntimeError(f"control find record exists but is not valid JSON: {path}")
        return None
    return FindRun.model_validate(payload)


def _require_matching_control_find(
    run: FindRun,
    expected_spec_hash: str,
    expected_prompt_hash: str,
) -> None:
    if run.manifest.spec_hash != expected_spec_hash:
        raise RuntimeError(
            f"control find record {run.find_run_id} spec_hash differs from current spec; "
            "refusing to reuse raw evidence"
        )
    if run.manifest.prompt_hash != expected_prompt_hash:
        raise RuntimeError(
            f"control find record {run.find_run_id} prompt_hash differs from current prompt; "
            "refusing to reuse raw evidence"
        )


def _require_infrastructure_failure(
    records_dir: Path,
    run_id: str,
    status: str,
    error: str | None,
) -> None:
    reason = _detect_record_infrastructure_failure(records_dir, run_id, status, error)
    if reason is None:
        raise RuntimeError(
            f"control find record {run_id} has status={status!r}, but is not a recognized "
            "infrastructure failure; refusing automatic replacement"
        )
    print(f"preserve {run_id} ({reason}); using recovery seed")


def _detect_record_infrastructure_failure(
    records_dir: Path,
    run_id: str,
    status: str,
    error: str | None,
) -> str | None:
    logs_dir = records_dir / f"{run_id}.logs"
    result = AgentResult(
        status=status,
        stdout=_read_if_exists(logs_dir / "stdout.txt"),
        stderr=_read_if_exists(logs_dir / "stderr.txt"),
        usage=UsageMetrics(),
        error=error,
    )
    return detect_infrastructure_failure(result)


def _read_if_exists(path: Path) -> str:
    if not path.exists():
        return ""
    return path.read_text(encoding="utf-8", errors="replace")


def _control_build_id(
    task_id: str,
    condition: str,
) -> str:
    short = hashlib.sha256(f"{task_id}|{condition}".encode()).hexdigest()[:8]
    return f"control-build-{condition}-{task_id}-{short}"


def _control_find_id(build_run_id: str, finder_id: str, trial_id: int, seed: int) -> str:
    short = hashlib.sha256(f"{build_run_id}|{finder_id}|{trial_id}|{seed}".encode()).hexdigest()[:8]
    return f"control-find-{build_run_id[:28]}-{finder_id}-t{trial_id}-s{seed}-{short}"


def _control_prompt_hash(condition: str, task_id: str) -> str:
    return hashlib.sha256(
        json.dumps(
            {
                "condition": condition,
                "task_id": task_id,
            },
            sort_keys=True,
        ).encode("utf-8")
    ).hexdigest()[:16]


if __name__ == "__main__":
    os.environ.setdefault("BNF_NO_ISOLATION", "0")
    main()
