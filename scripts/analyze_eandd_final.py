"""Analyze the frozen E&D final from append-only raw records.

The general analysis tables intentionally include every historical record.
For paper claims we need a stricter logical view of the final run:

* two scratch tasks;
* twelve final high/low agents;
* two builder trials;
* three finder trials;
* spec-only and question-only controls.

This script rebuilds that logical panel from raw JSON and chooses one
successful record per logical cell. Older pilots and preserved
infrastructure failures remain on disk but do not enter the final panel.
"""

# ruff: noqa: E402

from __future__ import annotations

import csv
import hashlib
import json
import math
import statistics
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from bnf.build.runner import _mk_id as _mk_build_id
from bnf.config import paths
from bnf.find.runner import _mk_id as _mk_find_id
from bnf.schema.run import BuildRun, FindRun, load_run_json

DEFAULT_TASKS = "scratch_minidb,scratch_nanoweb"
DEFAULT_AGENTS = ",".join(
    [
        "claude_code_opus_4_7_high",
        "claude_code_opus_4_7_low",
        "claude_code_sonnet_4_6_high",
        "claude_code_sonnet_4_6_low",
        "codex_gpt5_5_high",
        "codex_gpt5_5_low",
        "codex_gpt5_4_mini_high",
        "codex_gpt5_4_mini_low",
        "claude_code_mimo_v2_5_pro_high",
        "claude_code_mimo_v2_5_pro_low",
        "claude_code_mimo_v2_5_high",
        "claude_code_mimo_v2_5_low",
    ]
)
BUILD_TRIALS = 2
FIND_TRIALS = 3
PLANNED_SEED = 20260501
RECOVERY_SEED = 20261501
CONTROL_CONDITIONS = ("spec_only", "question_only")


@dataclass(frozen=True)
class BuildCell:
    task_id: str
    builder_id: str
    builder_trial_id: int
    seed: int
    run: BuildRun
    path: Path


@dataclass(frozen=True)
class BuildExclusion:
    task_id: str
    builder_id: str
    builder_trial_id: int
    seed: int
    reason: str
    run: BuildRun
    path: Path

    def as_build_cell(self) -> BuildCell:
        return BuildCell(
            task_id=self.task_id,
            builder_id=self.builder_id,
            builder_trial_id=self.builder_trial_id,
            seed=self.seed,
            run=self.run,
            path=self.path,
        )


@dataclass(frozen=True)
class FindCell:
    task_id: str
    build_run_id: str
    builder_id: str
    builder_trial_id: int
    finder_id: str
    finder_trial_id: int
    run: FindRun
    path: Path
    duplicate_ok_records: int


@dataclass(frozen=True)
class ControlFindCell:
    task_id: str
    condition: str
    build_run_id: str
    finder_id: str
    finder_trial_id: int
    run: FindRun
    path: Path
    duplicate_ok_records: int


def main() -> None:
    p = paths()
    out_dir = p.reports_dir / "eandd_final"
    out_dir.mkdir(parents=True, exist_ok=True)

    task_ids = _split_csv(DEFAULT_TASKS)
    agent_ids = _split_csv(DEFAULT_AGENTS)

    builds, build_exclusions = _load_final_build_panel(p.runs_dir, task_ids, agent_ids)
    finds = _load_final_finds(p.runs_dir, builds, agent_ids)
    controls = _load_control_finds(p.runs_dir, task_ids, agent_ids)

    _write_finder_summary(out_dir / "finder_summary.csv", finds, controls)
    _write_builder_summary(out_dir / "builder_summary.csv", finds, builds)
    _write_build_exclusions(out_dir / "build_exclusions.csv", build_exclusions)
    _write_builder_finder_matrix(out_dir / "builder_finder_matrix.csv", finds)
    _write_control_summary(out_dir / "control_summary.csv", controls)
    _write_question_summary(out_dir / "question_summary.csv", finds, controls)
    stability_summary, question_stability = _stability_tables(finds, controls)
    _write_stability_summary(out_dir / "stability_summary.csv", stability_summary)
    _write_question_stability(out_dir / "question_stability.csv", question_stability)
    _write_task_summary(out_dir / "task_summary.csv", finds, controls)
    metrics = _build_metrics(
        builds,
        finds,
        controls,
        task_ids,
        agent_ids,
        build_exclusions=build_exclusions,
        out_dir=out_dir,
    )
    (out_dir / "metrics.json").write_text(
        json.dumps(metrics, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    _write_markdown_report(out_dir / "insights.md", metrics)
    print(f"wrote {out_dir / 'insights.md'}")


def _split_csv(value: str) -> list[str]:
    return [part.strip() for part in value.split(",") if part.strip()]


def _control_build_id(task_id: str, condition: str) -> str:
    short = hashlib.sha256(f"{task_id}|{condition}".encode()).hexdigest()[:8]
    return f"control-build-{condition}-{task_id}-{short}"


def _control_find_id(build_run_id: str, finder_id: str, trial_id: int, seed: int) -> str:
    short = hashlib.sha256(f"{build_run_id}|{finder_id}|{trial_id}|{seed}".encode()).hexdigest()[:8]
    return f"control-find-{build_run_id[:28]}-{finder_id}-t{trial_id}-s{seed}-{short}"


def _load_final_builds(records_dir: Path, task_ids: list[str], agent_ids: list[str]) -> list[BuildCell]:
    builds, _exclusions = _load_final_build_panel(records_dir, task_ids, agent_ids)
    return builds


def _load_final_build_panel(
    records_dir: Path,
    task_ids: list[str],
    agent_ids: list[str],
) -> tuple[list[BuildCell], list[BuildExclusion]]:
    cells: list[BuildCell] = []
    exclusions: list[BuildExclusion] = []
    missing: list[str] = []
    for task_id in task_ids:
        for builder_id in agent_ids:
            for trial_id in range(BUILD_TRIALS):
                planned_seed = PLANNED_SEED + trial_id
                candidates = _ok_build_candidates(
                    records_dir,
                    task_id,
                    builder_id,
                    trial_id,
                    planned_seed,
                )
                if not candidates:
                    missing.append(_mk_build_id(task_id, builder_id, trial_id, planned_seed))
                    continue
                eligible = [
                    (seed, build, path)
                    for seed, build, path in candidates
                    if _build_primary_eligible(build)
                ]
                if not eligible:
                    seed, build, path = candidates[0]
                    exclusions.append(
                        BuildExclusion(
                            task_id=task_id,
                            builder_id=builder_id,
                            builder_trial_id=trial_id,
                            seed=seed,
                            reason=_build_exclusion_reason(build),
                            run=build,
                            path=path,
                        )
                    )
                    continue
                seed, build, path = eligible[0]
                cells.append(
                    BuildCell(
                        task_id=task_id,
                        builder_id=builder_id,
                        builder_trial_id=trial_id,
                        seed=seed,
                        run=build,
                        path=path,
                    )
                )
    if missing:
        details = []
        details.append(f"missing builds: {missing[:8]} ({len(missing)} total)")
        raise SystemExit("; ".join(details))
    return cells, exclusions


def _ok_build_candidates(
    records_dir: Path,
    task_id: str,
    builder_id: str,
    trial_id: int,
    planned_seed: int,
) -> list[tuple[int, BuildRun, Path]]:
    seeds = [planned_seed] + [RECOVERY_SEED + trial_id + offset * 1000 for offset in range(100)]
    candidates: list[tuple[int, BuildRun, Path]] = []
    for seed in seeds:
        run_id = _mk_build_id(task_id, builder_id, trial_id, seed)
        path = records_dir / f"{run_id}.json"
        payload = load_run_json(path)
        if payload is None:
            continue
        run = BuildRun.model_validate(payload)
        if run.status == "ok":
            candidates.append((seed, run, path))
    return candidates


def _build_primary_eligible(build: BuildRun) -> bool:
    return (
        build.status == "ok"
        and Path(build.artifact_path).exists()
        and build.validation.compile_pass
    )


def _build_exclusion_reason(build: BuildRun) -> str:
    if build.status != "ok":
        return f"status_{build.status}"
    if not Path(build.artifact_path).exists():
        return "artifact_missing"
    if not build.validation.compile_pass:
        return "compile_probe_failed"
    return "unknown"


def _load_final_finds(
    records_dir: Path,
    builds: list[BuildCell],
    agent_ids: list[str],
) -> list[FindCell]:
    runs_by_id = _load_find_records(records_dir, prefix="find-")
    cells: list[FindCell] = []
    missing: list[str] = []
    by_build = {build.run.build_run_id: build for build in builds}
    for build in builds:
        for finder_id in agent_ids:
            for trial_id in range(FIND_TRIALS):
                planned_seed = PLANNED_SEED + trial_id
                candidates: list[tuple[int, str, FindRun, Path]] = []
                for seed in [planned_seed] + [
                    RECOVERY_SEED + trial_id + offset * 1000 for offset in range(100)
                ]:
                    run_id = _mk_find_id(build.run.build_run_id, finder_id, trial_id, seed)
                    record = runs_by_id.get(run_id)
                    if record is not None and record[0].status == "ok":
                        candidates.append((seed, run_id, record[0], record[1]))
                if not candidates:
                    missing.append(_mk_find_id(build.run.build_run_id, finder_id, trial_id, planned_seed))
                    continue
                seed, _run_id, run, path = sorted(
                    candidates,
                    key=lambda item: (
                        item[0] >= RECOVERY_SEED,
                        item[0],
                        _created_at_sort_key(item[2]),
                        item[3].stat().st_mtime_ns,
                    ),
                )[-1]
                build_for_run = by_build[run.build_run_id]
                cells.append(
                    FindCell(
                        task_id=run.task_id,
                        build_run_id=run.build_run_id,
                        builder_id=build_for_run.builder_id,
                        builder_trial_id=build_for_run.builder_trial_id,
                        finder_id=finder_id,
                        finder_trial_id=trial_id,
                        run=run,
                        path=path,
                        duplicate_ok_records=len(candidates),
                    )
                )
    if missing:
        raise SystemExit(f"missing final find cells: {missing[:8]} ({len(missing)} total)")
    return cells


def _load_control_finds(
    records_dir: Path,
    task_ids: list[str],
    agent_ids: list[str],
) -> list[ControlFindCell]:
    runs_by_id = _load_find_records(records_dir, prefix="control-find-")
    cells: list[ControlFindCell] = []
    missing: list[str] = []
    for task_id in task_ids:
        for condition in CONTROL_CONDITIONS:
            build_run_id = _control_build_id(task_id, condition)
            for finder_id in agent_ids:
                for trial_id in range(FIND_TRIALS):
                    planned_seed = PLANNED_SEED + trial_id
                    candidates: list[tuple[int, str, FindRun, Path]] = []
                    for seed in [planned_seed] + [
                        RECOVERY_SEED + trial_id + offset * 1000 for offset in range(100)
                    ]:
                        run_id = _control_find_id(build_run_id, finder_id, trial_id, seed)
                        record = runs_by_id.get(run_id)
                        if record is not None and record[0].status == "ok":
                            candidates.append((seed, run_id, record[0], record[1]))
                    if not candidates:
                        missing.append(_control_find_id(build_run_id, finder_id, trial_id, planned_seed))
                        continue
                    _seed, _run_id, run, path = sorted(
                        candidates,
                        key=lambda item: (
                            item[0] >= RECOVERY_SEED,
                            item[0],
                            _created_at_sort_key(item[2]),
                            item[3].stat().st_mtime_ns,
                        ),
                    )[-1]
                    cells.append(
                        ControlFindCell(
                            task_id=task_id,
                            condition=condition,
                            build_run_id=build_run_id,
                            finder_id=finder_id,
                            finder_trial_id=trial_id,
                            run=run,
                            path=path,
                            duplicate_ok_records=len(candidates),
                        )
                    )
    if missing:
        raise SystemExit(f"missing control find cells: {missing[:8]} ({len(missing)} total)")
    return cells


def _load_find_records(records_dir: Path, *, prefix: str) -> dict[str, tuple[FindRun, Path]]:
    records: dict[str, tuple[FindRun, Path]] = {}
    for path in records_dir.glob(f"{prefix}*.json"):
        payload = load_run_json(path)
        if payload is None or "answers" not in payload:
            continue
        run = FindRun.model_validate(payload)
        records[run.find_run_id] = (run, path)
    return records


def _created_at_sort_key(run: FindRun) -> str:
    return run.manifest.created_at or ""


def _usage_total_tokens(run: BuildRun | FindRun) -> int:
    usage = run.usage
    vendor = usage.vendor_usage
    if vendor is not None and vendor.total_tokens:
        return vendor.total_tokens
    efficiency = usage.efficiency
    if efficiency is not None and efficiency.vendor_tokens is not None:
        return efficiency.vendor_tokens.total_tokens
    return 0


def _usage_cost(run: BuildRun | FindRun) -> float:
    usage = run.usage
    vendor = usage.vendor_usage
    if vendor is not None and vendor.cost_usd is not None:
        return vendor.cost_usd
    return 0.0


def _accuracy(run: FindRun) -> float:
    return sum(answer.is_correct for answer in run.answers) / len(run.answers)


def _answer_rows(cells: list[FindCell] | list[ControlFindCell]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for cell in cells:
        for answer in cell.run.answers:
            row = {
                "task_id": cell.task_id,
                "finder_id": cell.finder_id,
                "finder_trial_id": cell.finder_trial_id,
                "question_id": answer.question_id,
                "is_correct": int(answer.is_correct),
                "selected_option": answer.selected_option or "",
                "correct_option": answer.presented_correct_key,
            }
            if isinstance(cell, FindCell):
                row.update(
                    {
                        "build_run_id": cell.build_run_id,
                        "builder_id": cell.builder_id,
                        "builder_trial_id": cell.builder_trial_id,
                    }
                )
            else:
                row.update(
                    {
                        "build_run_id": cell.build_run_id,
                        "condition": cell.condition,
                    }
                )
            rows.append(row)
    return rows


def _mean(values: list[float] | list[int]) -> float:
    if not values:
        return math.nan
    return statistics.fmean(values)


def _median(values: list[float] | list[int]) -> float:
    if not values:
        return math.nan
    return statistics.median(values)


def _normal_ci95(values: list[float]) -> tuple[float, float]:
    if not values:
        return math.nan, math.nan
    if len(values) == 1:
        return values[0], values[0]
    mean = statistics.fmean(values)
    se = statistics.stdev(values) / math.sqrt(len(values))
    delta = 1.96 * se
    return max(0.0, mean - delta), min(1.0, mean + delta)


def _agent_base(agent_id: str) -> str:
    for suffix in ("_high", "_low"):
        if agent_id.endswith(suffix):
            return agent_id[: -len(suffix)]
    return agent_id


def _agent_effort(agent_id: str) -> str:
    for suffix in ("_high", "_low"):
        if agent_id.endswith(suffix):
            return suffix[1:]
    return "default"


def _agent_family(agent_id: str) -> str:
    if agent_id.startswith("codex_"):
        return "codex"
    if agent_id.startswith("claude_code_mimo_"):
        return "mimo"
    if agent_id.startswith("claude_code_"):
        return "claude"
    return agent_id.split("_", 1)[0]


def _format_float(value: float, digits: int = 3) -> str:
    if math.isnan(value):
        return ""
    return f"{value:.{digits}f}"


def _format_percent(value: float, digits: int = 1) -> str:
    if math.isnan(value):
        return ""
    return f"{value * 100:.{digits}f}%"


def _format_percentage_points(value: float, digits: int = 1) -> str:
    if math.isnan(value):
        return ""
    return f"{value * 100:+.{digits}f} pp"


def _write_csv(path: Path, rows: list[dict[str, Any]], fieldnames: list[str]) -> None:
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(rows)


def _write_finder_summary(
    path: Path,
    finds: list[FindCell],
    controls: list[ControlFindCell],
) -> None:
    formal_rows = _answer_rows(finds)
    control_rows = _answer_rows(controls)
    by_finder: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in formal_rows:
        by_finder[row["finder_id"]].append(row)
    control_by_key: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in control_rows:
        control_by_key[(row["finder_id"], row["condition"])].append(row)
    runs_by_finder: dict[str, list[FindCell]] = defaultdict(list)
    for cell in finds:
        runs_by_finder[cell.finder_id].append(cell)
    stable_by_finder = _control_stability_by_finder(controls)

    rows = []
    for finder_id in sorted(by_finder):
        formal_acc = _mean([r["is_correct"] for r in by_finder[finder_id]])
        spec_acc = _mean([r["is_correct"] for r in control_by_key[(finder_id, "spec_only")]])
        prior_acc = _mean([r["is_correct"] for r in control_by_key[(finder_id, "question_only")]])
        spec_stable, spec_total = stable_by_finder[(finder_id, "spec_only")]
        prior_stable, prior_total = stable_by_finder[(finder_id, "question_only")]
        run_accs = [_accuracy(cell.run) for cell in runs_by_finder[finder_id]]
        ci_low, ci_high = _normal_ci95(run_accs)
        tokens = [_usage_total_tokens(cell.run) for cell in runs_by_finder[finder_id]]
        costs = [_usage_cost(cell.run) for cell in runs_by_finder[finder_id]]
        rows.append(
            {
                "finder_id": finder_id,
                "family": _agent_family(finder_id),
                "base_model": _agent_base(finder_id),
                "effort": _agent_effort(finder_id),
                "formal_accuracy": _format_float(formal_acc, 6),
                "formal_run_ci95_low": _format_float(ci_low, 6),
                "formal_run_ci95_high": _format_float(ci_high, 6),
                "spec_only_accuracy": _format_float(spec_acc, 6),
                "question_only_accuracy": _format_float(prior_acc, 6),
                "formal_minus_spec_only": _format_float(formal_acc - spec_acc, 6),
                "formal_minus_question_only": _format_float(formal_acc - prior_acc, 6),
                "spec_only_stable_questions": spec_stable,
                "spec_only_total_questions": spec_total,
                "question_only_stable_questions": prior_stable,
                "question_only_total_questions": prior_total,
                "median_find_tokens": int(_median([v for v in tokens if v > 0]) or 0),
                "mean_find_tokens": int(_mean([v for v in tokens if v > 0]) or 0),
                "total_find_tokens": sum(tokens),
                "total_find_cost_usd": _format_float(sum(costs), 4),
                "find_runs": len(runs_by_finder[finder_id]),
                "question_rows": len(by_finder[finder_id]),
            }
        )
    rows.sort(key=lambda row: float(row["formal_accuracy"]), reverse=True)
    _write_csv(
        path,
        rows,
        [
            "finder_id",
            "family",
            "base_model",
            "effort",
            "formal_accuracy",
            "formal_run_ci95_low",
            "formal_run_ci95_high",
            "spec_only_accuracy",
            "question_only_accuracy",
            "formal_minus_spec_only",
            "formal_minus_question_only",
            "spec_only_stable_questions",
            "spec_only_total_questions",
            "question_only_stable_questions",
            "question_only_total_questions",
            "median_find_tokens",
            "mean_find_tokens",
            "total_find_tokens",
            "total_find_cost_usd",
            "find_runs",
            "question_rows",
        ],
    )


def _control_stability_by_finder(controls: list[ControlFindCell]) -> dict[tuple[str, str], tuple[int, int]]:
    grouped: dict[tuple[str, str, str, str], list[int]] = defaultdict(list)
    for cell in controls:
        for answer in cell.run.answers:
            grouped[(cell.finder_id, cell.condition, cell.task_id, answer.question_id)].append(
                int(answer.is_correct)
            )
    by_finder: dict[tuple[str, str], list[int]] = defaultdict(list)
    for (finder_id, condition, _task_id, _question_id), values in grouped.items():
        by_finder[(finder_id, condition)].append(int(len(values) == FIND_TRIALS and sum(values) == FIND_TRIALS))
    return {
        key: (sum(values), len(values))
        for key, values in by_finder.items()
    }


def _write_builder_summary(path: Path, finds: list[FindCell], builds: list[BuildCell]) -> None:
    answer_rows = _answer_rows(finds)
    by_builder: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in answer_rows:
        by_builder[row["builder_id"]].append(row)
    find_cells_by_builder: dict[str, list[FindCell]] = defaultdict(list)
    for cell in finds:
        find_cells_by_builder[cell.builder_id].append(cell)
    builds_by_builder: dict[str, list[BuildCell]] = defaultdict(list)
    for build in builds:
        builds_by_builder[build.builder_id].append(build)
    rows = []
    for builder_id in sorted(by_builder):
        tokens = [_usage_total_tokens(cell.run) for cell in builds_by_builder[builder_id]]
        costs = [_usage_cost(cell.run) for cell in builds_by_builder[builder_id]]
        ci_low, ci_high = _normal_ci95([_accuracy(cell.run) for cell in find_cells_by_builder[builder_id]])
        rows.append(
            {
                "builder_id": builder_id,
                "family": _agent_family(builder_id),
                "base_model": _agent_base(builder_id),
                "effort": _agent_effort(builder_id),
                "downstream_accuracy": _format_float(
                    _mean([row["is_correct"] for row in by_builder[builder_id]]), 6
                ),
                "downstream_run_ci95_low": _format_float(ci_low, 6),
                "downstream_run_ci95_high": _format_float(ci_high, 6),
                "median_build_tokens": int(_median([v for v in tokens if v > 0]) or 0),
                "mean_build_tokens": int(_mean([v for v in tokens if v > 0]) or 0),
                "total_build_tokens": sum(tokens),
                "total_build_cost_usd": _format_float(sum(costs), 4),
                "build_runs": len(builds_by_builder[builder_id]),
                "question_rows": len(by_builder[builder_id]),
            }
        )
    rows.sort(key=lambda row: float(row["downstream_accuracy"]), reverse=True)
    _write_csv(
        path,
        rows,
        [
            "builder_id",
            "family",
            "base_model",
            "effort",
            "downstream_accuracy",
            "downstream_run_ci95_low",
            "downstream_run_ci95_high",
            "median_build_tokens",
            "mean_build_tokens",
            "total_build_tokens",
            "total_build_cost_usd",
            "build_runs",
            "question_rows",
        ],
    )


def _write_build_exclusions(path: Path, exclusions: list[BuildExclusion]) -> None:
    rows = [
        {
            "build_run_id": exclusion.run.build_run_id,
            "task_id": exclusion.task_id,
            "builder_id": exclusion.builder_id,
            "builder_trial_id": exclusion.builder_trial_id,
            "seed": exclusion.seed,
            "reason": exclusion.reason,
            "compile_pass": int(exclusion.run.validation.compile_pass),
            "rust_files": exclusion.run.validation.rust_files,
            "rust_loc": exclusion.run.validation.rust_loc,
            "artifact_path": exclusion.run.artifact_path,
            "artifact_exists": int(Path(exclusion.run.artifact_path).exists()),
            "record_path": _display_path(exclusion.path),
        }
        for exclusion in exclusions
    ]
    _write_csv(
        path,
        rows,
        [
            "build_run_id",
            "task_id",
            "builder_id",
            "builder_trial_id",
            "seed",
            "reason",
            "compile_pass",
            "rust_files",
            "rust_loc",
            "artifact_path",
            "artifact_exists",
            "record_path",
        ],
    )


def _write_builder_finder_matrix(path: Path, finds: list[FindCell]) -> None:
    rows_by_key: dict[tuple[str, str], list[int]] = defaultdict(list)
    for cell in finds:
        rows_by_key[(cell.builder_id, cell.finder_id)].extend(
            int(answer.is_correct) for answer in cell.run.answers
        )
    rows = []
    for (builder_id, finder_id), values in sorted(rows_by_key.items()):
        rows.append(
            {
                "builder_id": builder_id,
                "finder_id": finder_id,
                "builder_family": _agent_family(builder_id),
                "finder_family": _agent_family(finder_id),
                "same_family": int(_agent_family(builder_id) == _agent_family(finder_id)),
                "same_base_model": int(_agent_base(builder_id) == _agent_base(finder_id)),
                "accuracy": _format_float(_mean(values), 6),
                "question_rows": len(values),
            }
        )
    _write_csv(
        path,
        rows,
        [
            "builder_id",
            "finder_id",
            "builder_family",
            "finder_family",
            "same_family",
            "same_base_model",
            "accuracy",
            "question_rows",
        ],
    )


def _write_control_summary(path: Path, controls: list[ControlFindCell]) -> None:
    rows_by_key: dict[tuple[str, str, str], list[int]] = defaultdict(list)
    for cell in controls:
        rows_by_key[(cell.condition, cell.task_id, cell.finder_id)].extend(
            int(answer.is_correct) for answer in cell.run.answers
        )
    rows = []
    for (condition, task_id, finder_id), values in sorted(rows_by_key.items()):
        rows.append(
            {
                "condition": condition,
                "task_id": task_id,
                "finder_id": finder_id,
                "accuracy": _format_float(_mean(values), 6),
                "question_rows": len(values),
            }
        )
    _write_csv(path, rows, ["condition", "task_id", "finder_id", "accuracy", "question_rows"])


def _write_question_summary(
    path: Path,
    finds: list[FindCell],
    controls: list[ControlFindCell],
) -> None:
    formal: dict[tuple[str, str], list[int]] = defaultdict(list)
    control: dict[tuple[str, str, str], list[int]] = defaultdict(list)
    for cell in finds:
        for answer in cell.run.answers:
            formal[(cell.task_id, answer.question_id)].append(int(answer.is_correct))
    for cell in controls:
        for answer in cell.run.answers:
            control[(cell.condition, cell.task_id, answer.question_id)].append(int(answer.is_correct))
    rows = []
    for task_q, values in sorted(formal.items()):
        task_id, question_id = task_q
        spec = _mean(control[("spec_only", task_id, question_id)])
        prior = _mean(control[("question_only", task_id, question_id)])
        formal_acc = _mean(values)
        rows.append(
            {
                "task_id": task_id,
                "question_id": question_id,
                "formal_accuracy": _format_float(formal_acc, 6),
                "spec_only_accuracy": _format_float(spec, 6),
                "question_only_accuracy": _format_float(prior, 6),
                "formal_minus_spec_only": _format_float(formal_acc - spec, 6),
                "formal_minus_question_only": _format_float(formal_acc - prior, 6),
                "formal_question_rows": len(values),
            }
        )
    _write_csv(
        path,
        rows,
        [
            "task_id",
            "question_id",
            "formal_accuracy",
            "spec_only_accuracy",
            "question_only_accuracy",
            "formal_minus_spec_only",
            "formal_minus_question_only",
            "formal_question_rows",
        ],
    )


def _stability_tables(
    finds: list[FindCell],
    controls: list[ControlFindCell],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Return three-trial repeatability summaries for formal and controls.

    A positive cell is a three-trial group that answers a question correctly in
    all three trials. For controls the group is (condition, task, question,
    finder). For artifact-present runs the group is (task, question, builder,
    build trial, finder).
    """

    answer_values: dict[tuple[str, str, str], list[int]] = defaultdict(list)
    group_values: dict[tuple[str, str, str, tuple[Any, ...]], list[int]] = defaultdict(list)
    for cell in controls:
        for answer in cell.run.answers:
            key = (cell.condition, cell.task_id, answer.question_id)
            group_key = (cell.finder_id,)
            value = int(answer.is_correct)
            answer_values[key].append(value)
            group_values[(*key, group_key)].append(value)
    for cell in finds:
        for answer in cell.run.answers:
            key = ("with_codebase", cell.task_id, answer.question_id)
            group_key = (cell.builder_id, cell.builder_trial_id, cell.finder_id)
            value = int(answer.is_correct)
            answer_values[key].append(value)
            group_values[(*key, group_key)].append(value)

    question_rows = []
    for key, values in sorted(answer_values.items()):
        condition, task_id, question_id = key
        groups = [group for group_key, group in group_values.items() if group_key[:3] == key]
        stable_cells = sum(1 for group in groups if len(group) == FIND_TRIALS and sum(group) == FIND_TRIALS)
        total_cells = len(groups)
        stable_rate = stable_cells / total_cells if total_cells else math.nan
        answer_accuracy = _mean(values)
        question_rows.append(
            {
                "condition": condition,
                "task_id": task_id,
                "question_id": question_id,
                "answer_accuracy": answer_accuracy,
                "answer_rows": len(values),
                "stable_cells": stable_cells,
                "total_cells": total_cells,
                "stable_cell_rate": stable_rate,
                "stable_ge_090": int(stable_rate >= 0.90),
                "all_attempts_correct": int(sum(values) == len(values)),
                "majority_stable": int(stable_cells > total_cells / 2),
            }
        )

    summary_rows = []
    for condition in ("with_codebase", "spec_only", "question_only"):
        rows_for_condition = [row for row in question_rows if row["condition"] == condition]
        task_ids = sorted({row["task_id"] for row in rows_for_condition})
        summary_rows.append(_stability_summary_row(condition, "all", rows_for_condition))
        for task_id in task_ids:
            task_rows = [row for row in rows_for_condition if row["task_id"] == task_id]
            summary_rows.append(_stability_summary_row(condition, task_id, task_rows))
    return summary_rows, question_rows


def _stability_summary_row(condition: str, task_id: str, rows: list[dict[str, Any]]) -> dict[str, Any]:
    answer_rows = sum(row["answer_rows"] for row in rows)
    correct_answers = sum(row["answer_accuracy"] * row["answer_rows"] for row in rows)
    stable_cells = sum(row["stable_cells"] for row in rows)
    total_cells = sum(row["total_cells"] for row in rows)
    return {
        "condition": condition,
        "task_id": task_id,
        "question_count": len(rows),
        "answer_rows": answer_rows,
        "answer_accuracy": correct_answers / answer_rows if answer_rows else math.nan,
        "stable_cells": stable_cells,
        "total_cells": total_cells,
        "stable_cell_rate": stable_cells / total_cells if total_cells else math.nan,
        "stable_questions_ge_090": sum(row["stable_cell_rate"] >= 0.90 for row in rows),
        "all_attempts_correct_questions": sum(row["all_attempts_correct"] for row in rows),
        "majority_stable_questions": sum(row["majority_stable"] for row in rows),
    }


def _write_stability_summary(path: Path, rows: list[dict[str, Any]]) -> None:
    _write_csv(
        path,
        [
            {
                **row,
                "answer_accuracy": _format_float(row["answer_accuracy"], 6),
                "stable_cell_rate": _format_float(row["stable_cell_rate"], 6),
            }
            for row in rows
        ],
        [
            "condition",
            "task_id",
            "question_count",
            "answer_rows",
            "answer_accuracy",
            "stable_cells",
            "total_cells",
            "stable_cell_rate",
            "stable_questions_ge_090",
            "all_attempts_correct_questions",
            "majority_stable_questions",
        ],
    )


def _write_question_stability(path: Path, rows: list[dict[str, Any]]) -> None:
    _write_csv(
        path,
        [
            {
                **row,
                "answer_accuracy": _format_float(row["answer_accuracy"], 6),
                "stable_cell_rate": _format_float(row["stable_cell_rate"], 6),
            }
            for row in rows
        ],
        [
            "condition",
            "task_id",
            "question_id",
            "answer_accuracy",
            "answer_rows",
            "stable_cells",
            "total_cells",
            "stable_cell_rate",
            "stable_ge_090",
            "all_attempts_correct",
            "majority_stable",
        ],
    )


def _write_task_summary(
    path: Path,
    finds: list[FindCell],
    controls: list[ControlFindCell],
) -> None:
    formal: dict[str, list[int]] = defaultdict(list)
    control: dict[tuple[str, str], list[int]] = defaultdict(list)
    for cell in finds:
        formal[cell.task_id].extend(int(answer.is_correct) for answer in cell.run.answers)
    for cell in controls:
        control[(cell.condition, cell.task_id)].extend(int(answer.is_correct) for answer in cell.run.answers)
    rows = []
    for task_id, values in sorted(formal.items()):
        spec = _mean(control[("spec_only", task_id)])
        prior = _mean(control[("question_only", task_id)])
        formal_acc = _mean(values)
        rows.append(
            {
                "task_id": task_id,
                "formal_accuracy": _format_float(formal_acc, 6),
                "spec_only_accuracy": _format_float(spec, 6),
                "question_only_accuracy": _format_float(prior, 6),
                "formal_question_rows": len(values),
            }
        )
    _write_csv(
        path,
        rows,
        ["task_id", "formal_accuracy", "spec_only_accuracy", "question_only_accuracy", "formal_question_rows"],
    )


def _build_metrics(
    builds: list[BuildCell],
    finds: list[FindCell],
    controls: list[ControlFindCell],
    task_ids: list[str],
    agent_ids: list[str],
    *,
    build_exclusions: list[BuildExclusion],
    out_dir: Path,
) -> dict[str, Any]:
    formal_answers = _answer_rows(finds)
    control_answers = _answer_rows(controls)
    total_formal_tokens = sum(_usage_total_tokens(cell.run) for cell in finds) + sum(
        _usage_total_tokens(cell.run) for cell in builds
    )
    total_control_tokens = sum(_usage_total_tokens(cell.run) for cell in controls)
    total_formal_cost = sum(_usage_cost(cell.run) for cell in finds) + sum(
        _usage_cost(cell.run) for cell in builds
    )
    total_control_cost = sum(_usage_cost(cell.run) for cell in controls)

    finder_summary = _summarize_by_agent(finds, controls, formal_answers, control_answers, role="finder")
    builder_summary = _summarize_builders(finds, builds, formal_answers)
    task_summary = _summarize_tasks(finds, controls)
    effort_deltas = _summarize_effort_deltas(finder_summary, builder_summary)
    affinity = _summarize_affinity(finds)
    questions = _summarize_questions(finds, controls)
    stability_summary, question_stability = _stability_tables(finds, controls)
    control_diagnostics = _summarize_control_diagnostics(questions["all_questions"])
    duplicates = {
        "formal_logical_cells_with_multiple_ok_records": sum(1 for cell in finds if cell.duplicate_ok_records > 1),
        "control_logical_cells_with_multiple_ok_records": sum(
            1 for cell in controls if cell.duplicate_ok_records > 1
        ),
    }
    planned_formal_build_cells = len(task_ids) * len(agent_ids) * BUILD_TRIALS
    expected_primary_find_cells = len(builds) * len(agent_ids) * FIND_TRIALS
    return {
        "generated_at": _source_snapshot_at(builds, finds, controls),
        "scope": {
            "tasks": task_ids,
            "agents": agent_ids,
            "build_trials": BUILD_TRIALS,
            "find_trials": FIND_TRIALS,
            "control_conditions": list(CONTROL_CONDITIONS),
            "planned_formal_build_cells": planned_formal_build_cells,
            "formal_build_cells": len(builds),
            "excluded_build_cells": len(build_exclusions),
            "excluded_build_reason_counts": dict(
                sorted(Counter(exclusion.reason for exclusion in build_exclusions).items())
            ),
            "formal_find_cells": len(finds),
            "formal_question_rows": len(formal_answers),
            "control_find_cells": len(controls),
            "control_question_rows": len(control_answers),
            "expected_formal_build_cells": len(builds),
            "planned_formal_find_cells": (
                planned_formal_build_cells * len(agent_ids) * FIND_TRIALS
            ),
            "expected_formal_find_cells": expected_primary_find_cells,
            "expected_control_find_cells": len(task_ids) * len(CONTROL_CONDITIONS) * len(agent_ids) * FIND_TRIALS,
        },
        "headline": {
            "formal_accuracy": _mean([row["is_correct"] for row in formal_answers]),
            "control_spec_only_accuracy": _mean(
                [row["is_correct"] for row in control_answers if row["condition"] == "spec_only"]
            ),
            "control_question_only_accuracy": _mean(
                [row["is_correct"] for row in control_answers if row["condition"] == "question_only"]
            ),
            "formal_total_tokens": total_formal_tokens,
            "control_total_tokens": total_control_tokens,
            "formal_total_cost_usd": total_formal_cost,
            "control_total_cost_usd": total_control_cost,
        },
        "stability_summary": stability_summary,
        "question_stability": question_stability,
        "finder_summary": finder_summary,
        "builder_summary": builder_summary,
        "task_summary": task_summary,
        "effort_deltas": effort_deltas,
        "affinity": affinity,
        "question_summary": questions,
        "control_diagnostics": control_diagnostics,
        "duplicates": duplicates,
        "outputs": {
            "finder_summary_csv": _display_path(out_dir / "finder_summary.csv"),
            "builder_summary_csv": _display_path(out_dir / "builder_summary.csv"),
            "build_exclusions_csv": _display_path(out_dir / "build_exclusions.csv"),
            "builder_finder_matrix_csv": _display_path(out_dir / "builder_finder_matrix.csv"),
            "control_summary_csv": _display_path(out_dir / "control_summary.csv"),
            "question_summary_csv": _display_path(out_dir / "question_summary.csv"),
            "stability_summary_csv": _display_path(out_dir / "stability_summary.csv"),
            "question_stability_csv": _display_path(out_dir / "question_stability.csv"),
            "task_summary_csv": _display_path(out_dir / "task_summary.csv"),
        },
    }


def _display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.resolve().as_posix()


def _source_snapshot_at(
    builds: list[BuildCell],
    finds: list[FindCell],
    controls: list[ControlFindCell],
) -> str:
    timestamps = [
        cell.run.manifest.created_at
        for cell in (*builds, *finds, *controls)
        if cell.run.manifest.created_at
    ]
    if timestamps:
        return max(timestamps)
    return datetime.fromtimestamp(0, timezone.utc).isoformat()


def _summarize_by_agent(
    finds: list[FindCell],
    controls: list[ControlFindCell],
    formal_answers: list[dict[str, Any]],
    control_answers: list[dict[str, Any]],
    *,
    role: str,
) -> list[dict[str, Any]]:
    assert role == "finder"
    by_agent: dict[str, list[int]] = defaultdict(list)
    by_control: dict[tuple[str, str], list[int]] = defaultdict(list)
    runs_by_agent: dict[str, list[FindCell]] = defaultdict(list)
    for row in formal_answers:
        by_agent[row["finder_id"]].append(row["is_correct"])
    for row in control_answers:
        by_control[(row["finder_id"], row["condition"])].append(row["is_correct"])
    for cell in finds:
        runs_by_agent[cell.finder_id].append(cell)
    stable_by_finder = _control_stability_by_finder(controls)
    out = []
    for agent_id, values in by_agent.items():
        spec = _mean(by_control[(agent_id, "spec_only")])
        prior = _mean(by_control[(agent_id, "question_only")])
        spec_stable, spec_total = stable_by_finder[(agent_id, "spec_only")]
        prior_stable, prior_total = stable_by_finder[(agent_id, "question_only")]
        formal = _mean(values)
        tokens = [_usage_total_tokens(cell.run) for cell in runs_by_agent[agent_id]]
        ci_low, ci_high = _normal_ci95([_accuracy(cell.run) for cell in runs_by_agent[agent_id]])
        out.append(
            {
                "agent_id": agent_id,
                "family": _agent_family(agent_id),
                "base_model": _agent_base(agent_id),
                "effort": _agent_effort(agent_id),
                "formal_accuracy": formal,
                "formal_run_ci95_low": ci_low,
                "formal_run_ci95_high": ci_high,
                "spec_only_accuracy": spec,
                "question_only_accuracy": prior,
                "formal_minus_spec_only": formal - spec,
                "formal_minus_question_only": formal - prior,
                "spec_only_stable_questions": spec_stable,
                "spec_only_total_questions": spec_total,
                "question_only_stable_questions": prior_stable,
                "question_only_total_questions": prior_total,
                "median_find_tokens": _median([v for v in tokens if v > 0]),
                "total_find_tokens": sum(tokens),
            }
        )
    return sorted(out, key=lambda row: row["formal_accuracy"], reverse=True)


def _summarize_builders(
    finds: list[FindCell],
    builds: list[BuildCell],
    formal_answers: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    by_agent: dict[str, list[int]] = defaultdict(list)
    builds_by_agent: dict[str, list[BuildCell]] = defaultdict(list)
    for row in formal_answers:
        by_agent[row["builder_id"]].append(row["is_correct"])
    find_cells_by_agent: dict[str, list[FindCell]] = defaultdict(list)
    for cell in finds:
        find_cells_by_agent[cell.builder_id].append(cell)
    for cell in builds:
        builds_by_agent[cell.builder_id].append(cell)
    out = []
    for agent_id, values in by_agent.items():
        tokens = [_usage_total_tokens(cell.run) for cell in builds_by_agent[agent_id]]
        ci_low, ci_high = _normal_ci95([_accuracy(cell.run) for cell in find_cells_by_agent[agent_id]])
        out.append(
            {
                "agent_id": agent_id,
                "family": _agent_family(agent_id),
                "base_model": _agent_base(agent_id),
                "effort": _agent_effort(agent_id),
                "downstream_accuracy": _mean(values),
                "downstream_run_ci95_low": ci_low,
                "downstream_run_ci95_high": ci_high,
                "median_build_tokens": _median([v for v in tokens if v > 0]),
                "total_build_tokens": sum(tokens),
            }
        )
    return sorted(out, key=lambda row: row["downstream_accuracy"], reverse=True)


def _summarize_tasks(finds: list[FindCell], controls: list[ControlFindCell]) -> list[dict[str, Any]]:
    formal: dict[str, list[int]] = defaultdict(list)
    control: dict[tuple[str, str], list[int]] = defaultdict(list)
    for cell in finds:
        formal[cell.task_id].extend(int(answer.is_correct) for answer in cell.run.answers)
    for cell in controls:
        control[(cell.condition, cell.task_id)].extend(int(answer.is_correct) for answer in cell.run.answers)
    out = []
    for task_id, values in formal.items():
        out.append(
            {
                "task_id": task_id,
                "formal_accuracy": _mean(values),
                "spec_only_accuracy": _mean(control[("spec_only", task_id)]),
                "question_only_accuracy": _mean(control[("question_only", task_id)]),
                "formal_rows": len(values),
            }
        )
    return sorted(out, key=lambda row: row["task_id"])


def _summarize_effort_deltas(
    finder_summary: list[dict[str, Any]],
    builder_summary: list[dict[str, Any]],
) -> dict[str, list[dict[str, Any]]]:
    return {
        "finder_high_minus_low": _effort_delta(finder_summary, "formal_accuracy"),
        "builder_high_minus_low": _effort_delta(builder_summary, "downstream_accuracy"),
    }


def _effort_delta(rows: list[dict[str, Any]], metric: str) -> list[dict[str, Any]]:
    by_base: dict[str, dict[str, dict[str, Any]]] = defaultdict(dict)
    for row in rows:
        by_base[row["base_model"]][row["effort"]] = row
    out = []
    for base, variants in sorted(by_base.items()):
        if "high" not in variants or "low" not in variants:
            continue
        out.append(
            {
                "base_model": base,
                "family": variants["high"]["family"],
                "high": variants["high"][metric],
                "low": variants["low"][metric],
                "high_minus_low": variants["high"][metric] - variants["low"][metric],
            }
        )
    return sorted(out, key=lambda row: row["high_minus_low"], reverse=True)


def _summarize_affinity(finds: list[FindCell]) -> dict[str, Any]:
    same_family: list[int] = []
    diff_family: list[int] = []
    same_base: list[int] = []
    diff_base: list[int] = []
    pair_values: dict[tuple[str, str], list[int]] = defaultdict(list)
    for cell in finds:
        values = [int(answer.is_correct) for answer in cell.run.answers]
        if _agent_family(cell.builder_id) == _agent_family(cell.finder_id):
            same_family.extend(values)
        else:
            diff_family.extend(values)
        if _agent_base(cell.builder_id) == _agent_base(cell.finder_id):
            same_base.extend(values)
        else:
            diff_base.extend(values)
        pair_values[(_agent_family(cell.builder_id), _agent_family(cell.finder_id))].extend(values)
    return {
        "same_family_accuracy": _mean(same_family),
        "different_family_accuracy": _mean(diff_family),
        "same_family_minus_different_family": _mean(same_family) - _mean(diff_family),
        "same_base_model_accuracy": _mean(same_base),
        "different_base_model_accuracy": _mean(diff_base),
        "same_base_model_minus_different_base_model": _mean(same_base) - _mean(diff_base),
        "family_pair_accuracy": [
            {
                "builder_family": key[0],
                "finder_family": key[1],
                "accuracy": _mean(values),
                "question_rows": len(values),
            }
            for key, values in sorted(pair_values.items())
        ],
    }


def _summarize_questions(
    finds: list[FindCell],
    controls: list[ControlFindCell],
) -> dict[str, Any]:
    formal: dict[tuple[str, str], list[int]] = defaultdict(list)
    control: dict[tuple[str, str, str], list[int]] = defaultdict(list)
    selected_options: dict[tuple[str, str], Counter[str]] = defaultdict(Counter)
    for cell in finds:
        for answer in cell.run.answers:
            key = (cell.task_id, answer.question_id)
            formal[key].append(int(answer.is_correct))
            selected_options[key][answer.selected_option or ""] += 1
    for cell in controls:
        for answer in cell.run.answers:
            control[(cell.condition, cell.task_id, answer.question_id)].append(int(answer.is_correct))
    rows = []
    for key, values in formal.items():
        task_id, question_id = key
        formal_acc = _mean(values)
        spec = _mean(control[("spec_only", task_id, question_id)])
        prior = _mean(control[("question_only", task_id, question_id)])
        top_selection = selected_options[key].most_common(1)[0] if selected_options[key] else ("", 0)
        rows.append(
            {
                "task_id": task_id,
                "question_id": question_id,
                "formal_accuracy": formal_acc,
                "spec_only_accuracy": spec,
                "question_only_accuracy": prior,
                "formal_minus_spec_only": formal_acc - spec,
                "formal_minus_question_only": formal_acc - prior,
                "top_selected_option": top_selection[0],
                "top_selected_share": top_selection[1] / len(values),
            }
        )
    rows.sort(key=lambda row: row["formal_accuracy"])
    return {
        "all_questions": rows,
        "hardest_formal_questions": rows[:8],
        "easiest_formal_questions": list(reversed(rows[-8:])),
        "most_spec_exposed_questions": sorted(rows, key=lambda row: row["spec_only_accuracy"], reverse=True)[:8],
        "largest_implementation_lift_questions": sorted(
            rows, key=lambda row: row["formal_minus_spec_only"], reverse=True
        )[:8],
        "weakest_implementation_lift_questions": sorted(
            rows, key=lambda row: row["formal_minus_spec_only"]
        )[:8],
        "largest_prior_lift_questions": sorted(
            rows, key=lambda row: row["formal_minus_question_only"], reverse=True
        )[:8],
        "weakest_prior_lift_questions": sorted(
            rows, key=lambda row: row["formal_minus_question_only"]
        )[:8],
    }


def _summarize_control_diagnostics(question_rows: list[dict[str, Any]]) -> dict[str, Any]:
    total = len(question_rows)
    if total == 0:
        return {}

    def count_if(predicate) -> int:
        return sum(1 for row in question_rows if predicate(row))

    return {
        "question_count": total,
        "spec_only_ge_090": count_if(lambda row: row["spec_only_accuracy"] >= 0.90),
        "question_only_ge_090": count_if(lambda row: row["question_only_accuracy"] >= 0.90),
        "both_controls_ge_090": count_if(
            lambda row: row["spec_only_accuracy"] >= 0.90
            and row["question_only_accuracy"] >= 0.90
        ),
        "formal_exceeds_spec_by_005": count_if(
            lambda row: row["formal_accuracy"] - row["spec_only_accuracy"] >= 0.05
        ),
        "formal_exceeds_question_only_by_005": count_if(
            lambda row: row["formal_accuracy"] - row["question_only_accuracy"] >= 0.05
        ),
        "formal_below_spec_by_005": count_if(
            lambda row: row["spec_only_accuracy"] - row["formal_accuracy"] >= 0.05
        ),
        "formal_below_question_only_by_005": count_if(
            lambda row: row["question_only_accuracy"] - row["formal_accuracy"] >= 0.05
        ),
    }


def _write_markdown_report(path: Path, metrics: dict[str, Any]) -> None:
    scope = metrics["scope"]
    headline = metrics["headline"]
    stability_all = {
        row["condition"]: row for row in metrics["stability_summary"] if row["task_id"] == "all"
    }
    stability_by_condition_task = {
        (row["condition"], row["task_id"]): row for row in metrics["stability_summary"]
    }
    lines = [
        "# E&D Final Logical Analysis",
        "",
        f"Generated: {metrics['generated_at']}",
        "",
        "## Scope Check",
        "",
        (
            "- Compile-pass primary formal builds: "
            f"{scope['formal_build_cells']} / {scope['expected_formal_build_cells']} "
            f"(planned {scope['planned_formal_build_cells']}; "
            f"excluded {scope['excluded_build_cells']})"
        ),
        f"- Build exclusions by reason: {scope['excluded_build_reason_counts']}",
        (
            "- Formal find records over compile-pass primary builds: "
            f"{scope['formal_find_cells']} / {scope['expected_formal_find_cells']} "
            f"(planned {scope['planned_formal_find_cells']})"
        ),
        f"- Formal question rows: {scope['formal_question_rows']}",
        f"- Control find records: {scope['control_find_cells']} / {scope['expected_control_find_cells']}",
        f"- Control question rows: {scope['control_question_rows']}",
        (
            "- Duplicate OK logical cells selected away: "
            f"formal={metrics['duplicates']['formal_logical_cells_with_multiple_ok_records']}, "
            f"control={metrics['duplicates']['control_logical_cells_with_multiple_ok_records']}"
        ),
        "",
        "This report is derived from raw JSON records by logical cell. It excludes older pilot records, preserved failed infrastructure attempts, status-ok builds whose release artifact path is missing, and compile-probe-failed builds from the primary estimands. Excluded builds and their find records remain in the release as validity-stress diagnostics.",
        "",
        "## Headline",
        "",
        "Three-trial agreement is the control headline; recovery accuracy is retained below for finder and builder diagnostics. Percentages are used for all rates in this report.",
        (
            "- Formal three-trial agreement: "
            f"{_format_percent(stability_all['with_codebase']['stable_cell_rate'])}; "
            "reliable-question rate: "
            f"{_format_percent(stability_all['with_codebase']['stable_questions_ge_090'] / stability_all['with_codebase']['question_count'])}"
        ),
        (
            "- Spec-only three-trial agreement: "
            f"{_format_percent(stability_all['spec_only']['stable_cell_rate'])}; "
            "reliable-question rate: "
            f"{_format_percent(stability_all['spec_only']['stable_questions_ge_090'] / stability_all['spec_only']['question_count'])}"
        ),
        (
            "- Question-only three-trial agreement: "
            f"{_format_percent(stability_all['question_only']['stable_cell_rate'])}; "
            "reliable-question rate: "
            f"{_format_percent(stability_all['question_only']['stable_questions_ge_090'] / stability_all['question_only']['question_count'])}"
        ),
        "",
        "## Finder Panel",
        "",
        "| finder | formal recovery | run CI95 | spec-only reliable questions | question-only reliable questions |",
        "| --- | ---: | ---: | ---: | ---: |",
    ]
    for row in metrics["finder_summary"]:
        lines.append(
            "| "
            + " | ".join(
                [
                    row["agent_id"],
                    _format_percent(row["formal_accuracy"]),
                    f"{_format_percent(row['formal_run_ci95_low'])}-{_format_percent(row['formal_run_ci95_high'])}",
                    _format_percent(
                        row["spec_only_stable_questions"] / row["spec_only_total_questions"]
                    ),
                    _format_percent(
                        row["question_only_stable_questions"]
                        / row["question_only_total_questions"]
                    ),
                ]
            )
            + " |"
        )
    lines.extend(
        [
            "",
            "## Top Builders",
            "",
            "| builder | downstream recovery | run CI95 |",
            "| --- | ---: | ---: |",
        ]
    )
    for row in metrics["builder_summary"][:8]:
        lines.append(
            f"| {row['agent_id']} | {_format_percent(row['downstream_accuracy'])} | {_format_percent(row['downstream_run_ci95_low'])}-{_format_percent(row['downstream_run_ci95_high'])} |"
        )
    lines.extend(
        [
            "",
            "## High vs Low Effort",
            "",
            "Positive values mean high effort outperformed low effort for the same base model.",
            "",
            "| role | base model | high-low delta |",
            "| --- | --- | ---: |",
        ]
    )
    for row in metrics["effort_deltas"]["finder_high_minus_low"]:
        lines.append(
            f"| finder | {row['base_model']} | {_format_percentage_points(row['high_minus_low'])} |"
        )
    for row in metrics["effort_deltas"]["builder_high_minus_low"]:
        lines.append(
            f"| builder | {row['base_model']} | {_format_percentage_points(row['high_minus_low'])} |"
        )
    affinity = metrics["affinity"]
    lines.extend(
        [
            "",
            "## Affinity",
            "",
            f"- Same-family builder/finder accuracy: {_format_percent(affinity['same_family_accuracy'])}",
            f"- Different-family builder/finder accuracy: {_format_percent(affinity['different_family_accuracy'])}",
            f"- Same-family minus different-family: {_format_percentage_points(affinity['same_family_minus_different_family'])}",
            f"- Same-base-model builder/finder accuracy: {_format_percent(affinity['same_base_model_accuracy'])}",
            f"- Different-base-model builder/finder accuracy: {_format_percent(affinity['different_base_model_accuracy'])}",
            f"- Same-base-model minus different-base-model: {_format_percentage_points(affinity['same_base_model_minus_different_base_model'])}",
            "",
            "## Task Repeatability",
            "",
            "| task | formal 3-trial agreement | spec-only 3-trial agreement | question-only 3-trial agreement | reliable-question rate, formal/spec/question |",
            "| --- | ---: | ---: | ---: | ---: |",
        ]
    )
    for task_id in sorted({row["task_id"] for row in metrics["task_summary"]}):
        formal = stability_by_condition_task[("with_codebase", task_id)]
        spec = stability_by_condition_task[("spec_only", task_id)]
        qonly = stability_by_condition_task[("question_only", task_id)]
        lines.append(
            f"| {task_id} | {_format_percent(formal['stable_cell_rate'])} | {_format_percent(spec['stable_cell_rate'])} | {_format_percent(qonly['stable_cell_rate'])} | {_format_percent(formal['stable_questions_ge_090'] / formal['question_count'])}/{_format_percent(spec['stable_questions_ge_090'] / spec['question_count'])}/{_format_percent(qonly['stable_questions_ge_090'] / qonly['question_count'])} |"
        )
    lines.extend(
        [
            "",
            "## Control Diagnostics",
            "",
        ]
    )
    lines.extend(
        [
            "| condition | 3-trial agreement | reliable-question rate |",
            "| --- | ---: | ---: |",
        ]
    )
    for condition in ("with_codebase", "spec_only", "question_only"):
        row = stability_all[condition]
        lines.append(
            f"| {condition} | {_format_percent(row['stable_cell_rate'])} | {_format_percent(row['stable_questions_ge_090'] / row['question_count'])} |"
        )
    lines.extend(
        [
            "",
            "## Compute Diagnostics",
            "",
            "Token and cost totals are reported only as run-cost diagnostics. They are not used as direct recovery-efficiency metrics because formal, spec-only, and question-only runs have different inputs.",
            "",
            f"- Formal tokens: {headline['formal_total_tokens']:,}",
            f"- Control tokens: {headline['control_total_tokens']:,}",
            f"- Formal cost USD: {_format_float(headline['formal_total_cost_usd'], 2)}",
            f"- Control cost USD: {_format_float(headline['control_total_cost_usd'], 2)}",
            "",
            "## Question Diagnostics",
            "",
            "Hardest formal questions:",
            "",
            "| task | question | formal recovery | spec-only recovery | question-only recovery |",
            "| --- | --- | ---: | ---: | ---: |",
        ]
    )
    for row in metrics["question_summary"]["hardest_formal_questions"]:
        lines.append(
            f"| {row['task_id']} | {row['question_id']} | {_format_percent(row['formal_accuracy'])} | {_format_percent(row['spec_only_accuracy'])} | {_format_percent(row['question_only_accuracy'])} |"
        )
    lines.extend(
        [
            "",
            "Largest artifact-conditioned lift over question-only:",
            "",
            "| task | question | formal-question lift | formal recovery | question-only recovery |",
            "| --- | --- | ---: | ---: | ---: |",
        ]
    )
    for row in metrics["question_summary"]["largest_prior_lift_questions"][:8]:
        lines.append(
            f"| {row['task_id']} | {row['question_id']} | {_format_percentage_points(row['formal_minus_question_only'])} | {_format_percent(row['formal_accuracy'])} | {_format_percent(row['question_only_accuracy'])} |"
        )
    lines.extend(
        [
            "",
            "## Reviewer-Risk Notes",
            "",
            "- Existing broad canonical tables include historical/pilot records; final paper claims should cite this logical report or regenerate paper tables from the same logical panel.",
            "- Spec-only accuracy is high enough to require careful wording: the benchmark measures recoverability of implementation intent, not every question being implementation-exclusive.",
            "- Finder orderings are near the ceiling, so report uncertainty, task split, and control-adjusted views instead of overclaiming small gaps.",
            "- Same-family affinity should be treated as a diagnostic until backed by confidence intervals or a mixed-effects model.",
            "",
            "## Output Files",
            "",
        ]
    )
    for key, value in metrics["outputs"].items():
        lines.append(f"- {key}: `{value}`")
    path.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
