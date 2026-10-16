"""Session runner for the option-free check.

Runs the 8 low-prior questions (the 10-question low-prior subset minus the
two ``negative_contrast`` items, which presuppose visible options) as
OPEN-ENDED questions — no options are shown — under two conditions:

* ``artifact_open``      — workspace is a clone of a released build
  artifact; free-form claims with REQUIRED file-level evidence.
* ``question_only_open`` — workspace contains only a ``CONTROL.md``
  marker; free-form claims from the question text and prior knowledge
  only, no evidence fields.

All records and forensics logs live under ``rebuttal/open_pilot/`` with
id prefix ``openfind-``. This script never writes into ``runs/`` or
``runs_release/`` (released artifacts are only read, as workspace
sources). Grading is manual, per the rubric fixed before the runs
(``rebuttal/open_pilot/rubric.md``). The sanitized release copy of the
records, rubric, and grading sheets is
``runs_release/reports/option_free_check/``, analyzed by
``scripts/analyze_option_free_check.py``.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import time
from dataclasses import dataclass
from pathlib import Path

from bnf.config import paths
from bnf.orchestration import WorkItem, WorkResult, run_work_queue
from bnf.prompts import finder_prompt_hash
from bnf.proxy import Gateway
from bnf.runners import AgentInvocation, AgentResult, get_agent
from bnf.runners.base import detect_infrastructure_failure
from bnf.runners.usage_trace import write_usage_events
from bnf.schema import BuildTaskSpec, FindQuestion, load_question_bank, load_specs_dir
from bnf.schema.run import AgentInfo, RunManifest, UsageMetrics, load_run_json

OPEN_CONDITIONS = ("artifact_open", "question_only_open")
OPEN_ANSWERS_FILENAME = "open_answers.json"
DEFAULT_FINDERS = "codex_gpt5_5_high,claude_code_opus_4_7_high"
DEFAULT_TRIALS = 3
DEFAULT_SEED = 20260724
DEFAULT_RECOVERY_SEED = 20267724

# The low-prior subset (question-only three-trial agreement < 0.90,
# `low_prior/manifest.json`) minus its two negative_contrast items
# (q_db_07, q_nw_11), which ask the finder to pick the INCONSISTENT
# option and therefore presuppose that options exist.
OPEN_QUESTION_IDS: dict[str, tuple[str, ...]] = {
    "scratch_minidb": ("q_db_01", "q_db_05", "q_db_06", "q_db_15"),
    "scratch_nanoweb": ("q_nw_01", "q_nw_04", "q_nw_07", "q_nw_10"),
}

# Default source artifact per task for the artifact_open condition.
#
# Selection rule, applied to the frozen release tables
# (runs_release/reports/artifact_conformance_consensus/final/
# artifact_conformance_final.csv joined with
# runs_release/tables/build_runs.csv):
#   1. compile_pass == true;
#   2. implemented_gold on 15/15 questions (full implementation coverage,
#      so open-ended gold claims are actually present in the code);
#   3. builder_family == "mimo" — a family different from BOTH default
#      finder families (codex / claude_code), so pilot results cannot be
#      attributed to builder-finder affinity;
#   4. if several artifacts remain, take the lexicographically first
#      build_run_id (deterministic; no cherry-picking).
# Eligible sets under rules 1-3 as of the frozen tables:
#   scratch_minidb : exactly one artifact qualifies —
#     build-scratch_minidb-claude_code_mimo_v2_5_low-t0-... (15/15; the
#     other five compile-pass MiMo minidb artifacts are 12-14/15).
#   scratch_nanoweb: two qualify —
#     build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-... and
#     build-scratch_nanoweb-claude_code_mimo_v2_5_pro_low-t1-...;
#     rule 4 picks the former.
DEFAULT_ARTIFACT_MAP: dict[str, str] = {
    "scratch_minidb": "build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3",
    "scratch_nanoweb": "build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601",
}


@dataclass(frozen=True)
class PilotPaths:
    """Filesystem layout of the pilot. Everything writable lives under
    ``rebuttal/open_pilot/``; ``runs/`` and ``runs_release/`` are never
    written to."""

    root: Path

    @property
    def pilot_dir(self) -> Path:
        return self.root / "rebuttal" / "open_pilot"

    @property
    def records_dir(self) -> Path:
        return self.pilot_dir / "records"

    @property
    def workspaces_dir(self) -> Path:
        return self.pilot_dir / "workspaces"

    @property
    def cargo_target_dir(self) -> Path:
        return self.pilot_dir / "cargo_target"

    @property
    def release_artifacts_dir(self) -> Path:
        return self.root / "runs_release" / "artifacts"


def main() -> None:
    args = _parse_args()
    p = paths()
    pilot = PilotPaths(root=p.root)
    specs = _selected_specs(p.specs_dir, args.tasks)
    finder_ids = _split_csv(args.finders)
    conditions = tuple(_split_csv(args.conditions))
    unknown_conditions = sorted(set(conditions) - set(OPEN_CONDITIONS))
    if unknown_conditions:
        raise SystemExit(f"unknown open-pilot condition(s): {', '.join(unknown_conditions)}")
    for finder_id in finder_ids:
        get_agent(finder_id)  # fail fast on unknown finder ids
    artifact_map = _load_artifact_map(pilot, args.artifact_map, specs, conditions)
    question_map = {
        spec.task_id: _selected_questions(p.questions_dir, spec.task_id) for spec in specs
    }

    if args.dry_run:
        _dry_run(pilot, specs, question_map, artifact_map, finder_ids, conditions, args)
        return

    gateway = None if args.no_proxy else Gateway.start()
    if gateway is not None:
        print(f"proxy listening on {gateway.base_url}")
    try:
        for spec in specs:
            _run_open_finds(
                pilot,
                spec,
                question_map[spec.task_id],
                artifact_map,
                finder_ids,
                conditions,
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
    parser.add_argument("--finders", default=DEFAULT_FINDERS)
    parser.add_argument("--conditions", default=",".join(OPEN_CONDITIONS))
    parser.add_argument("--trials", type=int, default=DEFAULT_TRIALS)
    parser.add_argument("--seed", type=int, default=DEFAULT_SEED)
    parser.add_argument("--recovery-seed", type=int, default=DEFAULT_RECOVERY_SEED)
    parser.add_argument("--workers", type=int, default=1)
    parser.add_argument("--no-proxy", action="store_true")
    parser.add_argument(
        "--artifact-map",
        type=Path,
        default=None,
        help="JSON file mapping task_id -> build_run_id under runs_release/artifacts "
        "(defaults to the pre-registered DEFAULT_ARTIFACT_MAP).",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Render and print all prompts and the planned run matrix; invoke nothing.",
    )
    return parser.parse_args()


def _split_csv(value: str) -> list[str]:
    return [part.strip() for part in value.split(",") if part.strip()]


def _selected_specs(specs_dir: Path, tasks: str) -> list[BuildTaskSpec]:
    wanted = set(_split_csv(tasks))
    unknown = wanted - set(OPEN_QUESTION_IDS)
    if unknown:
        raise SystemExit(f"task(s) outside the open pilot: {', '.join(sorted(unknown))}")
    specs = [spec for spec in load_specs_dir(specs_dir) if spec.task_id in wanted]
    missing = wanted - {spec.task_id for spec in specs}
    if missing:
        raise SystemExit(f"unknown task id(s): {', '.join(sorted(missing))}")
    return specs


def _selected_questions(questions_dir: Path, task_id: str) -> list[FindQuestion]:
    bank = load_question_bank(questions_dir / f"{task_id}.yaml")
    wanted = OPEN_QUESTION_IDS[task_id]
    by_id = {q.question_id: q for q in bank.questions}
    missing = [qid for qid in wanted if qid not in by_id]
    if missing:
        raise SystemExit(f"question(s) missing from {task_id} bank: {', '.join(missing)}")
    selected = [by_id[qid] for qid in wanted]
    contrast = [q.question_id for q in selected if q.question_type == "negative_contrast"]
    if contrast:
        raise SystemExit(
            f"negative_contrast question(s) selected for open pilot: {', '.join(contrast)}"
        )
    return selected


def _load_artifact_map(
    pilot: PilotPaths,
    map_path: Path | None,
    specs: list[BuildTaskSpec],
    conditions: tuple[str, ...],
) -> dict[str, str]:
    if map_path is None:
        mapping = dict(DEFAULT_ARTIFACT_MAP)
    else:
        raw = json.loads(map_path.read_text(encoding="utf-8"))
        if not isinstance(raw, dict) or not all(
            isinstance(k, str) and isinstance(v, str) for k, v in raw.items()
        ):
            raise SystemExit("--artifact-map must be a JSON object of task_id -> build_run_id")
        mapping = raw
    if "artifact_open" not in conditions:
        return mapping
    for spec in specs:
        build_run_id = mapping.get(spec.task_id)
        if build_run_id is None:
            raise SystemExit(f"artifact map has no build_run_id for task {spec.task_id}")
        artifact = pilot.release_artifacts_dir / build_run_id
        if not artifact.is_dir():
            raise SystemExit(f"released artifact not found: {artifact}")
    return mapping


# ----------------------------------------------------------------------
# Prompt rendering
# ----------------------------------------------------------------------


def _render_open_prompt(
    condition: str,
    spec: BuildTaskSpec,
    questions: list[FindQuestion],
) -> str:
    if condition == "artifact_open":
        preamble = [
            "You are examining a Rust codebase that has been placed in your",
            "current working directory. Read it — the file tree, `Cargo.toml`,",
            "any README, the source files, and the tests — and then answer the",
            "open-ended questions below in your own words.",
            "",
            "How to respond",
            "--------------",
            "- For each question, state a specific, factual claim (1-3",
            "  sentences) about what THIS codebase actually does. Commit to the",
            "  behaviour you observe; do not hedge across alternatives.",
            "- Evidence is REQUIRED for every answer: `evidence_files` must be",
            "  a non-empty list of repo-relative paths of the file(s) whose",
            "  code implements the behaviour you describe (source files, not",
            "  just README or docs). `evidence_symbols` (function, type, or",
            "  constant names) is optional but encouraged.",
            "- Do not include commentary in stdout. Your response is read from",
            "  a file you will create.",
            "",
            "Output file",
            "-----------",
            f"Write your answers to `{OPEN_ANSWERS_FILENAME}` at the workspace root as a",
            "single JSON object with this shape:",
            "",
            "    {",
            '      "answers": [',
            "        {",
            '          "question_id": "q_xxx",',
            '          "claim": "one to three sentences",',
            '          "evidence_files": ["src/foo.rs"],',
            '          "evidence_symbols": ["Foo::bar"],',
            '          "rationale": "one short sentence"',
            "        }",
            "      ]",
            "    }",
            "",
            "`question_id`, `claim`, and a non-empty `evidence_files` are",
            "required for every question. `evidence_symbols` and `rationale`",
            "are optional. Every question must appear in the array.",
        ]
    elif condition == "question_only_open":
        preamble = [
            "There is no codebase to inspect: your current working directory",
            "contains only a `CONTROL.md` marker file. Answer the open-ended",
            "questions below from the question text and your prior knowledge",
            "only.",
            "",
            "How to respond",
            "--------------",
            "- For each question, state a specific, factual claim (1-3",
            "  sentences) describing the single most likely design or",
            "  behaviour. Commit to one answer; do not hedge across",
            "  alternatives.",
            "- There is no repository to cite: omit `evidence_files` and",
            "  `evidence_symbols`, or leave them empty.",
            "- Do not include commentary in stdout. Your response is read from",
            "  a file you will create.",
            "",
            "Output file",
            "-----------",
            f"Write your answers to `{OPEN_ANSWERS_FILENAME}` at the workspace root as a",
            "single JSON object with this shape:",
            "",
            "    {",
            '      "answers": [',
            "        {",
            '          "question_id": "q_xxx",',
            '          "claim": "one to three sentences",',
            '          "rationale": "one short sentence"',
            "        }",
            "      ]",
            "    }",
            "",
            "`question_id` and `claim` are required for every question.",
            "`rationale` is optional. Every question must appear in the array.",
            "",
            f"Target task: {spec.task_id}",
        ]
    else:
        raise ValueError(condition)

    parts = list(preamble)
    parts.append("")
    parts.append(f"Total questions: {len(questions)}")
    parts.append("")
    for question in questions:
        parts.append(f"Question {question.question_id}")
        parts.append(question.prompt.strip())
        parts.append("")
    return "\n".join(parts).rstrip() + "\n"


# ----------------------------------------------------------------------
# Run execution
# ----------------------------------------------------------------------


def _run_open_finds(
    pilot: PilotPaths,
    spec: BuildTaskSpec,
    questions: list[FindQuestion],
    artifact_map: dict[str, str],
    finder_ids: list[str],
    conditions: tuple[str, ...],
    trials: int,
    seed: int,
    recovery_seed: int,
    workers: int,
    gateway: Gateway | None,
) -> None:
    items: list[WorkItem[dict]] = []
    for condition in conditions:
        prompt = _render_open_prompt(condition, spec, questions)
        build_run_id = artifact_map[spec.task_id] if condition == "artifact_open" else None
        for finder_id in finder_ids:
            for trial_id in range(trials):
                run_seed = _select_open_find_seed(
                    pilot,
                    spec,
                    condition,
                    finder_id,
                    trial_id,
                    prompt=prompt,
                    planned_seed=seed + trial_id,
                    recovery_seed=recovery_seed + trial_id,
                )
                if run_seed is None:
                    continue
                items.append(
                    WorkItem(
                        name=f"open {condition} {spec.task_id} {finder_id} t{trial_id}",
                        run=lambda condition=condition,
                        prompt=prompt,
                        build_run_id=build_run_id,
                        finder_id=finder_id,
                        trial_id=trial_id,
                        run_seed=run_seed: _run_one_open_find(
                            pilot,
                            spec,
                            questions,
                            condition,
                            prompt,
                            build_run_id,
                            finder_id,
                            trial_id,
                            run_seed,
                            gateway,
                        ),
                    )
                )
    run_work_queue(items, workers=workers, on_result=_print_open_progress)


def _run_one_open_find(
    pilot: PilotPaths,
    spec: BuildTaskSpec,
    questions: list[FindQuestion],
    condition: str,
    prompt: str,
    build_run_id: str | None,
    finder_id: str,
    trial_id: int,
    run_seed: int,
    gateway: Gateway | None,
) -> dict:
    adapter = get_agent(finder_id)
    run_id = _open_find_id(spec.task_id, condition, finder_id, trial_id, run_seed)
    workspace = _prepare_workspace(pilot, condition, spec, build_run_id, run_id)
    result = adapter.run(
        AgentInvocation(
            prompt=prompt,
            workspace=workspace,
            timeout_s=spec.constraints.build_time_budget_s,
            env={"CARGO_TARGET_DIR": str(pilot.cargo_target_dir.resolve())},
        ),
        gateway=gateway,
    )
    parsed = _read_open_answers(workspace)
    answers = []
    for question in questions:
        pick = parsed.get(question.question_id) or {}
        answers.append(
            {
                "question_id": question.question_id,
                "intention_id": question.intention_id,
                "question_prompt": question.prompt.strip(),
                "claim": pick.get("claim"),
                "evidence_files": pick.get("evidence_files") or [],
                "evidence_symbols": pick.get("evidence_symbols") or [],
                "rationale": pick.get("rationale"),
                "answered": bool(pick),
            }
        )
    manifest = RunManifest(
        run_id=run_id,
        spec_hash=spec.spec_hash(),
        prompt_hash=finder_prompt_hash(prompt),
        agent=AgentInfo(
            id=adapter.id,
            adapter=adapter.family,  # type: ignore[arg-type]
            model_id=adapter.model_id,
        ),
        seed=run_seed,
        trial_id=trial_id,
    )
    record = {
        "record_type": "open_find_run",
        "open_find_run_id": run_id,
        "condition": condition,
        "task_id": spec.task_id,
        "source_build_run_id": build_run_id,
        "question_ids": [q.question_id for q in questions],
        "manifest": json.loads(manifest.model_dump_json()),
        "answers": answers,
        "usage": json.loads(result.usage.model_dump_json()),
        "status": result.status,
        "error": result.error,
    }
    _write_record_atomic(record, pilot.records_dir)
    logs = pilot.records_dir / f"{run_id}.logs"
    logs.mkdir(parents=True, exist_ok=True)
    (logs / "prompt.txt").write_text(prompt, encoding="utf-8")
    (logs / "stdout.txt").write_text(result.stdout, encoding="utf-8")
    (logs / "stderr.txt").write_text(result.stderr, encoding="utf-8")
    write_usage_events(logs / "usage_events.jsonl", result.usage_events)
    answers_src = workspace / OPEN_ANSWERS_FILENAME
    if answers_src.exists():
        shutil.copy2(answers_src, logs / OPEN_ANSWERS_FILENAME)
    infra_reason = detect_infrastructure_failure(result)
    if infra_reason is not None:
        raise RuntimeError(
            f"infrastructure failure in {run_id}: {infra_reason}; "
            f"wrote raw evidence to {pilot.records_dir / f'{run_id}.json'} "
            f"and logs to {logs}"
        )
    return record


def _prepare_workspace(
    pilot: PilotPaths,
    condition: str,
    spec: BuildTaskSpec,
    build_run_id: str | None,
    run_id: str,
) -> Path:
    workspace = pilot.workspaces_dir / run_id
    if workspace.exists():
        shutil.rmtree(workspace)
    if condition == "artifact_open":
        assert build_run_id is not None
        shutil.copytree(
            pilot.release_artifacts_dir / build_run_id,
            workspace,
            ignore=shutil.ignore_patterns("target", ".git", "node_modules", ".venv", "__pycache__"),
        )
    else:
        workspace.mkdir(parents=True)
        (workspace / "CONTROL.md").write_text(
            f"condition: {condition}\ntarget_task_id: {spec.task_id}\n",
            encoding="utf-8",
        )
    return workspace


def _read_open_answers(workspace: Path) -> dict[str, dict]:
    """Read ``open_answers.json``; missing file or malformed JSON yields an
    empty mapping so the runner records "unanswered" rows rather than crash."""

    path = workspace / OPEN_ANSWERS_FILENAME
    if not path.exists():
        return {}
    try:
        obj = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}
    answers = obj.get("answers") if isinstance(obj, dict) else None
    if not isinstance(answers, list):
        return {}
    out: dict[str, dict] = {}
    for a in answers:
        if not isinstance(a, dict):
            continue
        qid = a.get("question_id")
        claim = a.get("claim")
        if not isinstance(qid, str) or not isinstance(claim, str) or not claim.strip():
            continue
        out[qid] = {
            "claim": claim.strip(),
            "evidence_files": _string_list(a.get("evidence_files")),
            "evidence_symbols": _string_list(a.get("evidence_symbols")),
            "rationale": a.get("rationale") if isinstance(a.get("rationale"), str) else None,
        }
    return out


def _string_list(value: object) -> list[str]:
    if not isinstance(value, list):
        return []
    return [item for item in value if isinstance(item, str) and item.strip()]


def _print_open_progress(result: WorkResult[dict], completed: int, total: int) -> None:
    record = result.value
    answered = sum(1 for a in record["answers"] if a["answered"])
    print(
        f"open {completed}/{total} {record['open_find_run_id']} "
        f"status={record['status']} answered={answered}/{len(record['answers'])} "
        f"elapsed={result.elapsed_seconds:.1f}s"
    )


# ----------------------------------------------------------------------
# Resume logic (mirrors run_controls.py: skip ok records, preserve raw
# evidence, replace only recognized infrastructure failures via a fresh
# recovery seed)
# ----------------------------------------------------------------------


def _select_open_find_seed(
    pilot: PilotPaths,
    spec: BuildTaskSpec,
    condition: str,
    finder_id: str,
    trial_id: int,
    *,
    prompt: str,
    planned_seed: int,
    recovery_seed: int,
) -> int | None:
    expected_prompt_hash = finder_prompt_hash(prompt)
    planned_id = _open_find_id(spec.task_id, condition, finder_id, trial_id, planned_seed)
    planned = _load_open_record(pilot.records_dir, planned_id)
    if planned is None:
        return planned_seed
    if planned.get("status") == "ok":
        _require_matching_record(planned, spec.spec_hash(), expected_prompt_hash)
        print(f"skip {planned_id}")
        return None
    _require_infrastructure_failure(pilot.records_dir, planned_id, planned)

    for attempt in range(100):
        seed = recovery_seed + attempt * 1000
        run_id = _open_find_id(spec.task_id, condition, finder_id, trial_id, seed)
        existing = _load_open_record(pilot.records_dir, run_id)
        if existing is None:
            return seed
        if existing.get("status") == "ok":
            _require_matching_record(existing, spec.spec_hash(), expected_prompt_hash)
            print(f"skip {run_id}")
            return None
        _require_infrastructure_failure(pilot.records_dir, run_id, existing)
    raise RuntimeError(f"no available recovery seed for {planned_id}")


def _load_open_record(records_dir: Path, run_id: str) -> dict | None:
    path = records_dir / f"{run_id}.json"
    payload = load_run_json(path)
    if payload is None:
        if path.exists():
            raise RuntimeError(f"open-pilot record exists but is not valid JSON: {path}")
        return None
    return payload


def _require_matching_record(
    record: dict,
    expected_spec_hash: str,
    expected_prompt_hash: str,
) -> None:
    run_id = record.get("open_find_run_id")
    manifest = record.get("manifest") or {}
    if manifest.get("spec_hash") != expected_spec_hash:
        raise RuntimeError(
            f"open-pilot record {run_id} spec_hash differs from current spec; "
            "refusing to reuse raw evidence"
        )
    if manifest.get("prompt_hash") != expected_prompt_hash:
        raise RuntimeError(
            f"open-pilot record {run_id} prompt_hash differs from current prompt; "
            "refusing to reuse raw evidence"
        )


def _require_infrastructure_failure(records_dir: Path, run_id: str, record: dict) -> None:
    logs_dir = records_dir / f"{run_id}.logs"
    result = AgentResult(
        status=str(record.get("status")),
        stdout=_read_if_exists(logs_dir / "stdout.txt"),
        stderr=_read_if_exists(logs_dir / "stderr.txt"),
        usage=UsageMetrics(),
        error=record.get("error"),
    )
    reason = detect_infrastructure_failure(result)
    if reason is None:
        raise RuntimeError(
            f"open-pilot record {run_id} has status={record.get('status')!r}, but is not a "
            "recognized infrastructure failure; refusing automatic replacement"
        )
    print(f"preserve {run_id} ({reason}); using recovery seed")


def _read_if_exists(path: Path) -> str:
    if not path.exists():
        return ""
    return path.read_text(encoding="utf-8", errors="replace")


# ----------------------------------------------------------------------
# Records
# ----------------------------------------------------------------------


def _open_find_id(
    task_id: str,
    condition: str,
    finder_id: str,
    trial_id: int,
    seed: int,
) -> str:
    short = hashlib.sha256(
        f"{task_id}|{condition}|{finder_id}|{trial_id}|{seed}".encode()
    ).hexdigest()[:8]
    return f"openfind-{task_id}-{condition}-{finder_id}-t{trial_id}-s{seed}-{short}"


def _write_record_atomic(record: dict, records_dir: Path) -> Path:
    """Atomic write (tmp + rename), mirroring bnf.schema.run.write_run."""

    records_dir.mkdir(parents=True, exist_ok=True)
    path = records_dir / f"{record['open_find_run_id']}.json"
    tmp = path.with_name(f"{path.name}.{os.getpid()}.{time.time_ns()}.tmp")
    tmp.write_text(json.dumps(record, indent=2), encoding="utf-8")
    os.replace(tmp, path)
    return path


# ----------------------------------------------------------------------
# Dry run
# ----------------------------------------------------------------------


def _dry_run(
    pilot: PilotPaths,
    specs: list[BuildTaskSpec],
    question_map: dict[str, list[FindQuestion]],
    artifact_map: dict[str, str],
    finder_ids: list[str],
    conditions: tuple[str, ...],
    args: argparse.Namespace,
) -> None:
    print("=== open pilot dry run (no agents invoked, nothing written) ===")
    for spec in specs:
        for condition in conditions:
            prompt = _render_open_prompt(condition, spec, question_map[spec.task_id])
            print()
            print(f"--- prompt for {spec.task_id} / {condition} "
                  f"(hash {finder_prompt_hash(prompt)}) ---")
            print(prompt, end="")
    print()
    print("--- planned run matrix ---")
    count = 0
    for spec in specs:
        for condition in conditions:
            source = (
                artifact_map[spec.task_id] if condition == "artifact_open" else "CONTROL.md only"
            )
            for finder_id in finder_ids:
                for trial_id in range(args.trials):
                    run_seed = args.seed + trial_id
                    run_id = _open_find_id(
                        spec.task_id, condition, finder_id, trial_id, run_seed
                    )
                    count += 1
                    print(f"{count:3d}  {run_id}")
                    print(f"     workspace source: {source}")
    print()
    print(
        f"total planned sessions: {count} "
        f"({len(finder_ids)} finders x {len(conditions)} conditions x "
        f"{args.trials} trials x {len(specs)} tasks)"
    )
    print(f"records dir: {pilot.records_dir}")
    print("existing ok records will be skipped at launch (resume logic)")


if __name__ == "__main__":
    os.environ.setdefault("BNF_NO_ISOLATION", "0")
    main()
