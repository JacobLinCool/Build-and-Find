"""Validity-stress diagnostics for the E&D final task pack.

This analysis is intentionally separate from the primary compile-pass logical
estimand. It reads canonical tables and reports how much recovery is available
from priors, from all artifact-present builds, from compile-passing primary
builds, and from stress slices such as compile-failed or empty artifacts.
"""

from __future__ import annotations

import csv
import json
import math
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from bnf.analysis.tables import (
    ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID,
    PAPER_FINAL_PANEL_ID,
    SCRATCH_TASKS,
    require_analysis_tables,
)
from bnf.config import paths


ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    p = paths()
    require_analysis_tables(p.tables_dir)
    out_dir = p.reports_dir / "eandd_final" / "validity_stress"
    out_dir.mkdir(parents=True, exist_ok=True)

    builds = _read_csv(p.tables_dir / "build_runs.csv")
    finds = _read_csv(p.tables_dir / "find_runs.csv")
    answers = _read_csv(p.tables_dir / "question_answers.csv")

    paper_builds = [
        row
        for row in builds
        if _in_panel(row, ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID)
        and row["task_id"] in SCRATCH_TASKS
    ]
    paper_build_by_id = {row["build_run_id"]: row for row in paper_builds}
    compile_pass_ids = {
        row["build_run_id"]
        for row in paper_builds
        if _as_bool(row["compile_pass"])
    }
    compile_failed_ids = set(paper_build_by_id) - compile_pass_ids
    empty_ids = {
        row["build_run_id"]
        for row in paper_builds
        if int(row["rust_files"]) == 0 or int(row["rust_loc"]) == 0
    }

    formal_find_rows = [
        row
        for row in finds
        if _in_panel(row, ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID)
        and row["build_run_id"] in paper_build_by_id
    ]
    formal_answer_rows = [
        row
        for row in answers
        if _in_panel(row, ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID)
        and row["build_run_id"] in paper_build_by_id
    ]
    question_only_rows = [
        row for row in answers if _in_panel(row, "question_only") and row["task_id"] in SCRATCH_TASKS
    ]

    qonly_by_task = _accuracy_by_task(question_only_rows)
    summary_rows: list[dict[str, Any]] = []
    summary_rows.extend(
        _summary_rows(
            "question_only_control",
            "Question-only control",
            [],
            [],
            question_only_rows,
            qonly_by_task,
        )
    )
    summary_rows.extend(
        _summary_rows(
            "artifact_present_all",
            "All artifact-present builds, including compile failures",
            paper_builds,
            formal_find_rows,
            formal_answer_rows,
            qonly_by_task,
        )
    )
    summary_rows.extend(
        _summary_rows(
            "compile_pass_primary",
            "Compile-pass primary panel",
            [paper_build_by_id[build_id] for build_id in sorted(compile_pass_ids)],
            [row for row in formal_find_rows if row["build_run_id"] in compile_pass_ids],
            [row for row in formal_answer_rows if row["build_run_id"] in compile_pass_ids],
            qonly_by_task,
        )
    )
    summary_rows.extend(
        _summary_rows(
            "compile_failed_excluded",
            "Compile-failed excluded diagnostic artifacts",
            [paper_build_by_id[build_id] for build_id in sorted(compile_failed_ids)],
            [row for row in formal_find_rows if row["build_run_id"] in compile_failed_ids],
            [row for row in formal_answer_rows if row["build_run_id"] in compile_failed_ids],
            qonly_by_task,
        )
    )
    summary_rows.extend(
        _summary_rows(
            "empty_artifact_excluded",
            "Observed empty excluded diagnostic artifact",
            [paper_build_by_id[build_id] for build_id in sorted(empty_ids)],
            [row for row in formal_find_rows if row["build_run_id"] in empty_ids],
            [row for row in formal_answer_rows if row["build_run_id"] in empty_ids],
            qonly_by_task,
        )
    )

    question_lift_rows = _question_lift_rows(
        [row for row in formal_answer_rows if row["build_run_id"] in compile_pass_ids],
        question_only_rows,
    )
    excluded_build_rows = _excluded_build_rows(
        [paper_build_by_id[build_id] for build_id in sorted(compile_failed_ids)],
        empty_ids,
    )

    _write_csv(out_dir / "stress_summary.csv", summary_rows)
    _write_csv(out_dir / "question_prior_lift.csv", question_lift_rows)
    _write_csv(out_dir / "excluded_builds.csv", excluded_build_rows)
    manifest = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "analysis_script": "scripts/analyze_eandd_validity_stress.py",
        "input_tables": {
            "build_runs": _display_path(p.tables_dir / "build_runs.csv"),
            "find_runs": _display_path(p.tables_dir / "find_runs.csv"),
            "question_answers": _display_path(p.tables_dir / "question_answers.csv"),
        },
        "artifact_present_panel": ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID,
        "primary_panel": PAPER_FINAL_PANEL_ID,
        "primary_rule": "compile_pass_primary excludes artifact-present builds whose compile probe failed",
        "diagnostic_rule": "artifact_present_all and excluded slices preserve release evidence for construct-validity stress tests",
    }
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    _write_markdown(out_dir / "validity_stress_insights.md", summary_rows, excluded_build_rows)
    print(f"wrote {out_dir / 'validity_stress_insights.md'}")


def _read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as fh:
        return list(csv.DictReader(fh))


def _write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    if not rows:
        path.write_text("", encoding="utf-8")
        return
    with path.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=list(rows[0]), lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


def _in_panel(row: dict[str, str], panel_id: str) -> bool:
    return panel_id in row.get("panel_ids", "").split("|")


def _as_bool(value: str) -> bool:
    return value.lower() in {"1", "true", "yes"}


def _accuracy(rows: list[dict[str, str]]) -> float:
    if not rows:
        return math.nan
    return sum(_as_bool(row["is_correct"]) for row in rows) / len(rows)


def _accuracy_by_task(rows: list[dict[str, str]]) -> dict[str, float]:
    out: dict[str, float] = {}
    for task_id in sorted({row["task_id"] for row in rows}):
        task_rows = [row for row in rows if row["task_id"] == task_id]
        out[task_id] = _accuracy(task_rows)
    out["all"] = _accuracy(rows)
    return out


def _perfect_rate(rows: list[dict[str, str]]) -> float:
    if not rows:
        return math.nan
    return sum(_as_bool(row["perfect_run"]) for row in rows) / len(rows)


def _summary_rows(
    slice_id: str,
    label: str,
    build_rows: list[dict[str, str]],
    find_rows: list[dict[str, str]],
    answer_rows: list[dict[str, str]],
    qonly_by_task: dict[str, float],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    task_ids = sorted({row["task_id"] for row in answer_rows})
    if not task_ids and slice_id == "question_only_control":
        task_ids = sorted(task_id for task_id in qonly_by_task if task_id != "all")
    for task_id in [*task_ids, "all"]:
        task_answers = answer_rows if task_id == "all" else [row for row in answer_rows if row["task_id"] == task_id]
        task_builds = build_rows if task_id == "all" else [row for row in build_rows if row["task_id"] == task_id]
        task_finds = find_rows if task_id == "all" else [row for row in find_rows if row["task_id"] == task_id]
        accuracy = _accuracy(task_answers)
        qonly = qonly_by_task.get(task_id, math.nan)
        rows.append(
            {
                "slice_id": slice_id,
                "label": label,
                "task_id": task_id,
                "build_runs": len(task_builds),
                "find_runs": len(task_finds),
                "question_rows": len(task_answers),
                "accuracy": _format_float(accuracy),
                "perfect_run_rate": _format_float(_perfect_rate(task_finds)),
                "question_only_accuracy": _format_float(qonly),
                "formal_minus_question_only": _format_float(accuracy - qonly),
            }
        )
    return rows


def _question_lift_rows(
    formal_rows: list[dict[str, str]],
    question_only_rows: list[dict[str, str]],
) -> list[dict[str, Any]]:
    formal_by_question: dict[tuple[str, str], list[dict[str, str]]] = defaultdict(list)
    qonly_by_question: dict[tuple[str, str], list[dict[str, str]]] = defaultdict(list)
    for row in formal_rows:
        formal_by_question[(row["task_id"], row["question_id"])].append(row)
    for row in question_only_rows:
        qonly_by_question[(row["task_id"], row["question_id"])].append(row)
    rows: list[dict[str, Any]] = []
    for key in sorted(formal_by_question):
        formal_acc = _accuracy(formal_by_question[key])
        qonly_acc = _accuracy(qonly_by_question[key])
        task_id, question_id = key
        rows.append(
            {
                "task_id": task_id,
                "question_id": question_id,
                "compile_pass_accuracy": _format_float(formal_acc),
                "question_only_accuracy": _format_float(qonly_acc),
                "compile_pass_minus_question_only": _format_float(formal_acc - qonly_acc),
                "compile_pass_question_rows": len(formal_by_question[key]),
                "question_only_rows": len(qonly_by_question[key]),
            }
        )
    return rows


def _excluded_build_rows(
    build_rows: list[dict[str, str]],
    empty_ids: set[str],
) -> list[dict[str, Any]]:
    return [
        {
            "build_run_id": row["build_run_id"],
            "task_id": row["task_id"],
            "builder_id": row["builder_id"],
            "build_trial_id": row["build_trial_id"],
            "compile_pass": row["compile_pass"],
            "rust_files": row["rust_files"],
            "rust_loc": row["rust_loc"],
            "empty_artifact": int(row["build_run_id"] in empty_ids),
            "artifact_path": row["artifact_path"],
        }
        for row in build_rows
    ]


def _write_markdown(
    path: Path,
    summary_rows: list[dict[str, Any]],
    excluded_build_rows: list[dict[str, Any]],
) -> None:
    by_key = {(row["slice_id"], row["task_id"]): row for row in summary_rows}
    lines = [
        "# Validity Stress Diagnostics",
        "",
        "This report separates the compile-pass primary panel from prior-only and stress diagnostics.",
        "",
        "## Main Stress Table",
        "",
        "| slice | task | builds | find runs | answer rows | accuracy | perfect-run rate | delta vs question-only |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for slice_id in (
        "question_only_control",
        "artifact_present_all",
        "compile_pass_primary",
        "compile_failed_excluded",
        "empty_artifact_excluded",
    ):
        for task_id in ("scratch_minidb", "scratch_nanoweb", "all"):
            row = by_key.get((slice_id, task_id))
            if row is None:
                continue
            lines.append(
                f"| {slice_id} | {task_id} | {row['build_runs']} | {row['find_runs']} | "
                f"{row['question_rows']} | {_format_percent(row['accuracy'])} | "
                f"{_format_percent(row['perfect_run_rate'])} | "
                f"{_format_percentage_points(row['formal_minus_question_only'])} |"
            )
    lines.extend(
        [
            "",
            "## Excluded Builds",
            "",
            "| task | builder | trial | rust files | rust LOC | empty | build run |",
            "| --- | --- | ---: | ---: | ---: | ---: | --- |",
        ]
    )
    for row in excluded_build_rows:
        lines.append(
            f"| {row['task_id']} | {row['builder_id']} | {row['build_trial_id']} | "
            f"{row['rust_files']} | {row['rust_loc']} | {row['empty_artifact']} | "
            f"{row['build_run_id']} |"
        )
    path.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")


def _format_float(value: float) -> str:
    if math.isnan(value):
        return ""
    return f"{value:.6f}"


def _format_percent(value: str) -> str:
    if value == "":
        return "--"
    return f"{float(value) * 100:.1f}%"


def _format_percentage_points(value: str) -> str:
    if value == "":
        return "--"
    return f"{float(value) * 100:+.1f} pp"


def _display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.resolve().as_posix()


if __name__ == "__main__":
    main()
