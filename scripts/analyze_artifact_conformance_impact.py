"""Analyze how artifact conformance labels affect paper metrics.

This script treats ``implemented_gold`` artifact-question pairs as the audited
evaluation set. Non-gold and ambiguous labels remain reported as builder-side
implementation outcomes instead of being silently folded into finder recovery
scores.
"""

# ruff: noqa: E402

from __future__ import annotations

import csv
import hashlib
import json
import math
import random
import statistics
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Literal

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src"
if str(SRC) not in sys.path:
    sys.path.insert(0, str(SRC))
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from bnf.config import paths  # noqa: E402


ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID = "artifact_present_diagnostic"
PAPER_FINAL_PANEL_ID = "paper_final_scratch"
SCRATCH_TASKS = frozenset({"scratch_minidb", "scratch_nanoweb"})
REQUIRED_TABLES = ("build_runs.csv", "find_runs.csv", "question_answers.csv")
FINAL_LABELS = (
    "implemented_gold",
    "implemented_non_gold",
    "absent_or_ambiguous",
    "documented_only",
    "not_applicable_or_invalid",
)
FIGURE1_EFFORT_METRIC = "retrieval_bytes"
FIGURE1_RECOVERY_VARIANT = "mean_answer_recovery"
ORIGINAL_FIGURE1_RECOVERY_VARIANT = "soft_full_recovery"
PRIMARY_FIGURE2_KEY = ("perfect_run", "total_bytes")
BOOTSTRAP_REPS = 2000
BOOTSTRAP_SEED = 7
Gate = Literal["all_runs", "perfect_run", "max_penalty"]

AGENT_ORDER = [
    "codex_gpt5_5_high",
    "codex_gpt5_4_mini_high",
    "claude_code_opus_4_7_high",
    "claude_code_sonnet_4_6_high",
    "claude_code_mimo_v2_5_pro_high",
    "claude_code_mimo_v2_5_high",
    "codex_gpt5_5_low",
    "codex_gpt5_4_mini_low",
    "claude_code_opus_4_7_low",
    "claude_code_sonnet_4_6_low",
    "claude_code_mimo_v2_5_pro_low",
    "claude_code_mimo_v2_5_low",
]


@dataclass(frozen=True)
class Panel:
    panel_id: str
    description: str
    source_panel_id: str
    effort: str
    primary: bool


def main() -> None:
    p = paths()
    _require_tables(p.tables_dir)

    final_dir = p.reports_dir / "artifact_conformance_consensus" / "final"
    final_path = final_dir / "artifact_conformance_final.csv"
    if not final_path.exists():
        raise SystemExit(
            f"missing {final_path}; run scripts/merge_artifact_conformance_annotations.py first"
        )

    out_dir = p.reports_dir / "artifact_conformance_consensus" / "impact"
    out_dir.mkdir(parents=True, exist_ok=True)

    find_rows = _read_csv(p.tables_dir / "find_runs.csv")
    answer_rows = _read_csv(p.tables_dir / "question_answers.csv")
    conformance_rows = _read_csv(final_path)
    label_by_key = {
        _artifact_question_key(row): row
        for row in conformance_rows
        if row["task_id"] in SCRATCH_TASKS
    }

    panels = _panels()
    gate_summary_rows = _conformance_gate_summary_rows(
        conformance_rows=conformance_rows,
        find_rows=find_rows,
        answer_rows=answer_rows,
    )
    accuracy_label_rows = _accuracy_by_label_rows(
        conformance_rows=conformance_rows,
        answer_rows=answer_rows,
    )
    builder_conformance_rows = _builder_conformance_rows(conformance_rows)

    conditioned_run_rows = _conditioned_run_rows(
        find_rows=find_rows,
        answer_rows=answer_rows,
        label_by_key=label_by_key,
        source_panel_id=PAPER_FINAL_PANEL_ID,
    )
    conditioned_answer_rows = _conditioned_answer_rows(
        answer_rows=answer_rows,
        label_by_key=label_by_key,
        source_panel_id=PAPER_FINAL_PANEL_ID,
    )
    low_prior_questions = _load_question_set(
        p.reports_dir / "eandd_final" / "low_prior" / "manifest.json",
        keys=("question_subset", "questions"),
    )
    pivot_manifest_path = p.reports_dir / "eandd_final" / "pivots" / "manifest.json"
    implementation_signal_questions = _load_question_set(
        pivot_manifest_path,
        keys=("implementation_signal_questions",),
    )
    low_prior_run_rows = _conditioned_run_rows(
        find_rows=find_rows,
        answer_rows=answer_rows,
        label_by_key=label_by_key,
        source_panel_id=PAPER_FINAL_PANEL_ID,
        question_filter=low_prior_questions,
    )
    low_prior_affinity_rows = _affinity_residual_rows(low_prior_run_rows)
    affinity_rows = _affinity_residual_rows(conditioned_run_rows)

    recovery_rows: list[dict[str, Any]] = []
    builder_score_rows: list[dict[str, Any]] = []
    rank_rows: list[dict[str, Any]] = []
    for panel in panels:
        panel_rows = _panel_run_rows(conditioned_run_rows, panel)
        recovery_rows.extend(_recovery_rows(panel, panel_rows, "retrieval_bytes"))
        recovery_rows.extend(_recovery_rows(panel, panel_rows, "total_bytes"))
        for gate in ("all_runs", "perfect_run", "max_penalty"):
            for metric in ("retrieval_bytes", "total_bytes", "tool_call_count"):
                means = _cell_means(panel_rows, metric=metric, gate=gate)
                builder_score_rows.extend(_builder_score_rows(panel, gate, metric, means))
                rank_rows.extend(_rank_stability_rows(panel, gate, metric, means))
    _add_builder_score_bootstrap_cis(builder_score_rows, panels, conditioned_run_rows)

    low_prior_recovery_rows: list[dict[str, Any]] = []
    low_prior_builder_score_rows: list[dict[str, Any]] = []
    low_prior_rank_rows: list[dict[str, Any]] = []
    for panel in panels:
        panel_rows = _panel_run_rows(low_prior_run_rows, panel)
        low_prior_recovery_rows.extend(_recovery_rows(panel, panel_rows, "retrieval_bytes"))
        low_prior_recovery_rows.extend(_recovery_rows(panel, panel_rows, "total_bytes"))
        for gate in ("all_runs", "perfect_run", "max_penalty"):
            for metric in ("retrieval_bytes", "total_bytes", "tool_call_count"):
                means = _cell_means(panel_rows, metric=metric, gate=gate)
                low_prior_builder_score_rows.extend(_builder_score_rows(panel, gate, metric, means))
                low_prior_rank_rows.extend(_rank_stability_rows(panel, gate, metric, means))
    _add_builder_score_bootstrap_cis(low_prior_builder_score_rows, panels, low_prior_run_rows)

    original_builder_summary = _read_csv(p.reports_dir / "eandd_final" / "builder_summary.csv")
    builder_summary_rows = _conditioned_builder_summary_rows(
        conditioned_run_rows,
        original_builder_summary=original_builder_summary,
        builder_conformance_rows=builder_conformance_rows,
    )
    agent_view_rows = _conditioned_agent_view_rows(
        conditioned_answer_rows,
        implementation_signal_questions=implementation_signal_questions,
        low_prior_questions=low_prior_questions,
    )

    comparison_rows = _score_comparison_rows(
        original_recovery_path=p.reports_dir
        / "eandd_final"
        / "within_finder_efficiency"
        / "recovery_scores.csv",
        original_builder_path=p.reports_dir
        / "eandd_final"
        / "within_finder_efficiency"
        / "builder_scores.csv",
        conditioned_recovery_rows=recovery_rows,
        conditioned_builder_rows=builder_score_rows,
    )

    _write_csv(out_dir / "conformance_gate_summary.csv", gate_summary_rows)
    _write_csv(out_dir / "accuracy_by_conformance_label.csv", accuracy_label_rows)
    _write_csv(out_dir / "builder_conformance_summary.csv", builder_conformance_rows)
    _write_csv(out_dir / "conditioned_answer_rows.csv", conditioned_answer_rows)
    _write_csv(out_dir / "conditioned_run_rows.csv", conditioned_run_rows)
    _write_csv(out_dir / "conditioned_builder_summary.csv", builder_summary_rows)
    _write_csv(out_dir / "conditioned_agent_views.csv", agent_view_rows)
    _write_csv(out_dir / "conditioned_recovery_scores.csv", recovery_rows)
    _write_csv(out_dir / "conditioned_builder_scores.csv", builder_score_rows)
    _write_csv(out_dir / "conditioned_rank_stability.csv", rank_rows)
    _write_csv(out_dir / "conditioned_affinity_residual_12x12.csv", affinity_rows)
    _write_csv(out_dir / "conditioned_affinity_residual_family.csv", _family_residual_rows(affinity_rows))
    _write_csv(out_dir / "low_prior_conditioned_run_rows.csv", low_prior_run_rows)
    _write_csv(out_dir / "low_prior_conditioned_recovery_scores.csv", low_prior_recovery_rows)
    _write_csv(out_dir / "low_prior_conditioned_builder_scores.csv", low_prior_builder_score_rows)
    _write_csv(out_dir / "low_prior_conditioned_rank_stability.csv", low_prior_rank_rows)
    _write_csv(out_dir / "low_prior_conditioned_affinity_residual_12x12.csv", low_prior_affinity_rows)
    _write_csv(
        out_dir / "low_prior_conditioned_affinity_residual_family.csv",
        _family_residual_rows(low_prior_affinity_rows),
    )
    _write_csv(out_dir / "score_comparison.csv", comparison_rows)

    manifest = {
        "analysis_id": "artifact_conformance_impact_v1",
        "analysis_script": "scripts/analyze_artifact_conformance_impact.py",
        "analysis_command": " ".join(sys.argv),
        "generated_at": datetime.now(UTC).isoformat(),
        "input_paths": [
            _display_path(p.tables_dir / "build_runs.csv"),
            _display_path(p.tables_dir / "find_runs.csv"),
            _display_path(p.tables_dir / "question_answers.csv"),
            _display_path(final_path),
        ],
        "input_hashes": {
            _display_path(path): _sha256(path)
            for path in (
                p.tables_dir / "build_runs.csv",
                p.tables_dir / "find_runs.csv",
                p.tables_dir / "question_answers.csv",
                final_path,
            )
        },
        "output_dir": _display_path(out_dir),
        "primary_panel_id": PAPER_FINAL_PANEL_ID,
        "artifact_present_panel_id": ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID,
        "conformance_condition": "Only artifact-question pairs labeled implemented_gold contribute to conditioned finder scores.",
        "conditioned_effort_rule": "Each conditioned run keeps the observed full-run inspection effort while recomputing accuracy/perfect-run status over implemented_gold questions only.",
        "row_counts": {
            "conformance_rows": len(conformance_rows),
            "conditioned_answer_rows": len(conditioned_answer_rows),
            "conditioned_run_rows": len(conditioned_run_rows),
            "low_prior_conditioned_run_rows": len(low_prior_run_rows),
            "gate_summary_rows": len(gate_summary_rows),
            "score_comparison_rows": len(comparison_rows),
        },
        "final_labels": dict(Counter(row["final_label"] for row in conformance_rows)),
    }
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    _write_markdown(
        out_dir / "impact_report.md",
        manifest=manifest,
        gate_summary_rows=gate_summary_rows,
        accuracy_label_rows=accuracy_label_rows,
        builder_conformance_rows=builder_conformance_rows,
        comparison_rows=comparison_rows,
    )
    print(f"wrote {out_dir / 'impact_report.md'}")


def _panels() -> list[Panel]:
    return [
        Panel(
            panel_id="high_effort",
            description="Primary high-effort panel conditioned on implemented-gold artifact questions",
            source_panel_id=PAPER_FINAL_PANEL_ID,
            effort="high",
            primary=True,
        ),
        Panel(
            panel_id="low_effort",
            description="Primary low-effort panel conditioned on implemented-gold artifact questions",
            source_panel_id=PAPER_FINAL_PANEL_ID,
            effort="low",
            primary=True,
        ),
    ]


def _read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as handle:
        return list(csv.DictReader(handle))


def _require_tables(tables_dir: Path) -> None:
    missing = [name for name in REQUIRED_TABLES if not (tables_dir / name).exists()]
    if missing:
        raise SystemExit(f"missing required analysis tables in {tables_dir}: {', '.join(missing)}")


def _load_question_set(path: Path, *, keys: tuple[str, ...]) -> set[tuple[str, str]]:
    data: Any = json.loads(path.read_text(encoding="utf-8"))
    for key in keys:
        data = data[key]
    questions = set()
    for value in data:
        task_id, question_id = value.split(":", 1)
        questions.add((task_id, question_id))
    if not questions:
        raise RuntimeError(f"empty question set at {path} keys={keys}")
    return questions


def _write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    if not rows:
        path.write_text("", encoding="utf-8")
        return
    fieldnames = list(rows[0].keys())
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(rows)


def _artifact_question_key(row: dict[str, str]) -> tuple[str, str, str]:
    artifact_id = row.get("artifact_id") or row["build_run_id"]
    return (row["task_id"], artifact_id, row["question_id"])


def _in_panel(row: dict[str, str], panel_id: str) -> bool:
    return panel_id in row.get("panel_ids", "").split("|")


def _as_bool(value: str | bool) -> bool:
    if isinstance(value, bool):
        return value
    return value.strip().lower() in {"1", "true", "yes"}


def _as_float(value: str) -> float:
    return float(value) if value not in {"", "nan", "NaN"} else math.nan


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


def _mean(values: list[float]) -> float:
    clean = [value for value in values if not math.isnan(value)]
    if not clean:
        return math.nan
    return sum(clean) / len(clean)


def _accuracy(rows: list[dict[str, str]]) -> float:
    if not rows:
        return math.nan
    return sum(_as_bool(row["is_correct"]) for row in rows) / len(rows)


def _perfect_rate(rows: list[dict[str, str]]) -> float:
    if not rows:
        return math.nan
    return sum(_as_bool(row["perfect_run"]) for row in rows) / len(rows)


def _scope_answer_rows(
    answer_rows: list[dict[str, str]],
    *,
    scope_id: str,
    build_ids: set[str],
) -> list[dict[str, str]]:
    if scope_id == "compile_pass_primary":
        panel_id = PAPER_FINAL_PANEL_ID
    else:
        panel_id = ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID
    return [
        row
        for row in answer_rows
        if row["build_run_id"] in build_ids
        and row["task_id"] in SCRATCH_TASKS
        and _in_panel(row, panel_id)
    ]


def _scope_find_rows(
    find_rows: list[dict[str, str]],
    *,
    scope_id: str,
    build_ids: set[str],
) -> list[dict[str, str]]:
    if scope_id == "compile_pass_primary":
        panel_id = PAPER_FINAL_PANEL_ID
    else:
        panel_id = ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID
    return [
        row
        for row in find_rows
        if row["build_run_id"] in build_ids
        and row["task_id"] in SCRATCH_TASKS
        and _in_panel(row, panel_id)
    ]


def _conformance_gate_summary_rows(
    *,
    conformance_rows: list[dict[str, str]],
    find_rows: list[dict[str, str]],
    answer_rows: list[dict[str, str]],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    scopes = (
        (
            "artifact_present_all",
            "All artifact-present diagnostic artifacts, including compile failures",
            [row for row in conformance_rows if row["task_id"] in SCRATCH_TASKS],
        ),
        (
            "compile_pass_primary",
            "Compile-pass primary panel used by the paper figures",
            [
                row
                for row in conformance_rows
                if row["task_id"] in SCRATCH_TASKS and _as_bool(row["compile_pass"])
            ],
        ),
        (
            "compile_failed_diagnostic",
            "Compile-failed artifact-present diagnostics",
            [
                row
                for row in conformance_rows
                if row["task_id"] in SCRATCH_TASKS and not _as_bool(row["compile_pass"])
            ],
        ),
    )
    for scope_id, scope_label, scope_rows in scopes:
        build_ids = {row["artifact_id"] for row in scope_rows}
        scope_answers = _scope_answer_rows(answer_rows, scope_id=scope_id, build_ids=build_ids)
        scope_finds = _scope_find_rows(find_rows, scope_id=scope_id, build_ids=build_ids)
        answers_by_label = _answers_by_conformance_label(scope_answers, scope_rows)
        for task_id in (*sorted(SCRATCH_TASKS), "all"):
            task_rows = (
                scope_rows if task_id == "all" else [row for row in scope_rows if row["task_id"] == task_id]
            )
            task_build_ids = {row["artifact_id"] for row in task_rows}
            task_answers = (
                scope_answers
                if task_id == "all"
                else [row for row in scope_answers if row["task_id"] == task_id]
            )
            task_finds = (
                scope_finds if task_id == "all" else [row for row in scope_finds if row["task_id"] == task_id]
            )
            task_answers_by_label = {
                label: [
                    row
                    for row in answers_by_label[label]
                    if task_id == "all" or row["task_id"] == task_id
                ]
                for label in FINAL_LABELS
            }
            counts = Counter(row["final_label"] for row in task_rows)
            implemented_gold_answers = task_answers_by_label["implemented_gold"]
            excluded_answers = [
                row
                for label in FINAL_LABELS
                if label != "implemented_gold"
                for row in task_answers_by_label[label]
            ]
            rows.append(
                {
                    "scope_id": scope_id,
                    "scope_label": scope_label,
                    "task_id": task_id,
                    "build_runs": len(task_build_ids),
                    "find_runs": len(task_finds),
                    "artifact_question_pairs": len(task_rows),
                    "implemented_gold": counts["implemented_gold"],
                    "implemented_gold_rate": _ratio(counts["implemented_gold"], len(task_rows)),
                    "implemented_non_gold": counts["implemented_non_gold"],
                    "implemented_non_gold_rate": _ratio(counts["implemented_non_gold"], len(task_rows)),
                    "absent_or_ambiguous": counts["absent_or_ambiguous"],
                    "absent_or_ambiguous_rate": _ratio(counts["absent_or_ambiguous"], len(task_rows)),
                    "other_non_primary_labels": sum(
                        counts[label]
                        for label in ("documented_only", "not_applicable_or_invalid")
                    ),
                    "excluded_pairs": len(task_rows) - counts["implemented_gold"],
                    "excluded_pair_rate": _ratio(len(task_rows) - counts["implemented_gold"], len(task_rows)),
                    "answer_rows_original": len(task_answers),
                    "answer_rows_implemented_gold": len(implemented_gold_answers),
                    "answer_rows_excluded": len(excluded_answers),
                    "accuracy_original": _accuracy(task_answers),
                    "accuracy_implemented_gold": _accuracy(implemented_gold_answers),
                    "accuracy_excluded": _accuracy(excluded_answers),
                    "perfect_run_rate_original": _perfect_rate(task_finds),
                }
            )
    return rows


def _accuracy_by_label_rows(
    *,
    conformance_rows: list[dict[str, str]],
    answer_rows: list[dict[str, str]],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for scope_id, panel_id, scope_rows in (
        (
            "artifact_present_all",
            ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID,
            [row for row in conformance_rows if row["task_id"] in SCRATCH_TASKS],
        ),
        (
            "compile_pass_primary",
            PAPER_FINAL_PANEL_ID,
            [
                row
                for row in conformance_rows
                if row["task_id"] in SCRATCH_TASKS and _as_bool(row["compile_pass"])
            ],
        ),
    ):
        build_ids = {row["artifact_id"] for row in scope_rows}
        scope_answers = [
            row
            for row in answer_rows
            if row["build_run_id"] in build_ids
            and row["task_id"] in SCRATCH_TASKS
            and _in_panel(row, panel_id)
        ]
        answers_by_label = _answers_by_conformance_label(scope_answers, scope_rows)
        for task_id in (*sorted(SCRATCH_TASKS), "all"):
            for label in FINAL_LABELS:
                label_answers = [
                    row
                    for row in answers_by_label[label]
                    if task_id == "all" or row["task_id"] == task_id
                ]
                label_pairs = [
                    row
                    for row in scope_rows
                    if row["final_label"] == label
                    and (task_id == "all" or row["task_id"] == task_id)
                ]
                rows.append(
                    {
                        "scope_id": scope_id,
                        "task_id": task_id,
                        "final_label": label,
                        "artifact_question_pairs": len(label_pairs),
                        "answer_rows": len(label_answers),
                        "accuracy": _accuracy(label_answers),
                    }
                )
    return rows


def _answers_by_conformance_label(
    answer_rows: list[dict[str, str]],
    conformance_rows: list[dict[str, str]],
) -> dict[str, list[dict[str, str]]]:
    label_by_key = {_artifact_question_key(row): row["final_label"] for row in conformance_rows}
    out: dict[str, list[dict[str, str]]] = {label: [] for label in FINAL_LABELS}
    missing = 0
    for row in answer_rows:
        label = label_by_key.get(_artifact_question_key(row))
        if label is None:
            missing += 1
            continue
        out[label].append(row)
    if missing:
        raise RuntimeError(f"{missing} answer rows had no conformance label")
    return out


def _builder_conformance_rows(conformance_rows: list[dict[str, str]]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    group_keys = [
        ("all", "all", "all"),
        *sorted(
            {
                (row["task_id"], row["compile_pass"], row["builder_id"])
                for row in conformance_rows
                if row["task_id"] in SCRATCH_TASKS
            }
        ),
    ]
    for task_id, compile_pass, builder_id in group_keys:
        if task_id == "all":
            group = [row for row in conformance_rows if row["task_id"] in SCRATCH_TASKS]
        else:
            group = [
                row
                for row in conformance_rows
                if row["task_id"] == task_id
                and row["compile_pass"] == compile_pass
                and row["builder_id"] == builder_id
            ]
        counts = Counter(row["final_label"] for row in group)
        rows.append(
            {
                "task_id": task_id,
                "compile_pass": compile_pass,
                "builder_id": builder_id,
                "builder_family": _agent_family(builder_id) if builder_id != "all" else "all",
                "builder_effort": _agent_effort(builder_id) if builder_id != "all" else "all",
                "artifact_question_pairs": len(group),
                "implemented_gold": counts["implemented_gold"],
                "implemented_gold_rate": _ratio(counts["implemented_gold"], len(group)),
                "implemented_non_gold": counts["implemented_non_gold"],
                "absent_or_ambiguous": counts["absent_or_ambiguous"],
                "excluded_pairs": len(group) - counts["implemented_gold"],
                "excluded_pair_rate": _ratio(len(group) - counts["implemented_gold"], len(group)),
            }
        )
    rows.sort(
        key=lambda row: (
            row["task_id"] != "all",
            row["task_id"],
            row["compile_pass"],
            -row["excluded_pair_rate"],
            row["builder_id"],
        )
    )
    return rows


def _conditioned_run_rows(
    *,
    find_rows: list[dict[str, str]],
    answer_rows: list[dict[str, str]],
    label_by_key: dict[tuple[str, str, str], dict[str, str]],
    source_panel_id: str,
    question_filter: set[tuple[str, str]] | None = None,
) -> list[dict[str, Any]]:
    answers_by_find: dict[str, list[dict[str, str]]] = defaultdict(list)
    original_answers_by_find: dict[str, list[dict[str, str]]] = defaultdict(list)
    for row in answer_rows:
        if not _in_panel(row, source_panel_id) or row["task_id"] not in SCRATCH_TASKS:
            continue
        if question_filter is not None and (row["task_id"], row["question_id"]) not in question_filter:
            continue
        original_answers_by_find[row["find_run_id"]].append(row)
        label = label_by_key.get(_artifact_question_key(row))
        if label is None:
            raise RuntimeError(f"missing conformance label for {row['question_row_id']}")
        if label["final_label"] == "implemented_gold":
            answers_by_find[row["find_run_id"]].append(row)

    rows: list[dict[str, Any]] = []
    for find in find_rows:
        if not _in_panel(find, source_panel_id) or find["task_id"] not in SCRATCH_TASKS:
            continue
        implemented_gold_answers = answers_by_find.get(find["find_run_id"], [])
        if not implemented_gold_answers:
            continue
        original_answers = original_answers_by_find[find["find_run_id"]]
        n_correct = sum(_as_bool(row["is_correct"]) for row in implemented_gold_answers)
        accuracy = n_correct / len(implemented_gold_answers)
        rows.append(
            {
                "find_run_id": find["find_run_id"],
                "build_run_id": find["build_run_id"],
                "task_id": find["task_id"],
                "builder_id": find["builder_id"],
                "builder_family": find["builder_family"],
                "builder_base_model": _agent_base(find["builder_id"]),
                "builder_effort": find["builder_effort"],
                "build_trial_id": int(find["build_trial_id"]),
                "finder_id": find["finder_id"],
                "finder_family": find["finder_family"],
                "finder_base_model": _agent_base(find["finder_id"]),
                "finder_effort": find["finder_effort"],
                "finder_trial_id": int(find["finder_trial_id"]),
                "accuracy": accuracy,
                "perfect_run": int(accuracy == 1.0),
                "retrieval_bytes": _as_float(find["retrieval_bytes"]),
                "tool_call_bytes": _as_float(find["tool_call_bytes"]),
                "total_bytes": _as_float(find["total_bytes"]),
                "tool_call_count": _as_float(find["tool_call_count"]),
                "turn_count": _as_float(find["turn_count"]),
                "wall_clock_seconds": _as_float(find["wall_clock_seconds"]),
                "question_count": len(implemented_gold_answers),
                "original_question_count": len(original_answers),
                "original_accuracy": _as_float(find["accuracy"]),
                "original_perfect_run": int(_as_bool(find["perfect_run"])),
                "excluded_question_count": len(original_answers) - len(implemented_gold_answers),
            }
        )
    return rows


def _conditioned_answer_rows(
    *,
    answer_rows: list[dict[str, str]],
    label_by_key: dict[tuple[str, str, str], dict[str, str]],
    source_panel_id: str,
    question_filter: set[tuple[str, str]] | None = None,
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for row in answer_rows:
        if not _in_panel(row, source_panel_id) or row["task_id"] not in SCRATCH_TASKS:
            continue
        if question_filter is not None and (row["task_id"], row["question_id"]) not in question_filter:
            continue
        label = label_by_key.get(_artifact_question_key(row))
        if label is None:
            raise RuntimeError(f"missing conformance label for {row['question_row_id']}")
        if label["final_label"] != "implemented_gold":
            continue
        rows.append(
            {
                "question_row_id": row["question_row_id"],
                "find_run_id": row["find_run_id"],
                "build_run_id": row["build_run_id"],
                "task_id": row["task_id"],
                "builder_id": row["builder_id"],
                "builder_family": _agent_family(row["builder_id"]),
                "builder_base_model": _agent_base(row["builder_id"]),
                "builder_effort": _agent_effort(row["builder_id"]),
                "finder_id": row["finder_id"],
                "finder_family": _agent_family(row["finder_id"]),
                "finder_base_model": _agent_base(row["finder_id"]),
                "finder_effort": _agent_effort(row["finder_id"]),
                "question_id": row["question_id"],
                "is_correct": int(_as_bool(row["is_correct"])),
            }
        )
    return rows


def _panel_run_rows(rows: list[dict[str, Any]], panel: Panel) -> list[dict[str, Any]]:
    return [
        row
        for row in rows
        if row["builder_effort"] == panel.effort and row["finder_effort"] == panel.effort
    ]


def _cell_means(
    rows: list[dict[str, Any]],
    *,
    metric: str,
    gate: Gate,
) -> dict[tuple[str, str, str], dict[str, Any]]:
    all_grouped: dict[tuple[str, str, str], list[dict[str, Any]]] = defaultdict(list)
    perfect_grouped: dict[tuple[str, str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        key = (row["task_id"], row["builder_id"], row["finder_id"])
        all_grouped[key].append(row)
        if row["perfect_run"]:
            perfect_grouped[key].append(row)

    all_means = {
        key: _metric_mean(group, metric)
        for key, group in all_grouped.items()
        if _metric_mean(group, metric) > 0
    }
    max_by_ft: dict[tuple[str, str], float] = defaultdict(float)
    for (task_id, _builder_id, finder_id), value in all_means.items():
        max_by_ft[(task_id, finder_id)] = max(max_by_ft[(task_id, finder_id)], value)

    out: dict[tuple[str, str, str], dict[str, Any]] = {}
    for key, group in all_grouped.items():
        task_id, builder_id, finder_id = key
        perfect_group = perfect_grouped.get(key, [])
        if gate == "all_runs":
            contributing = group
            cost = _metric_mean(contributing, metric)
            imputed = 0
        elif gate == "perfect_run":
            contributing = perfect_group
            if not contributing:
                continue
            cost = _metric_mean(contributing, metric)
            imputed = 0
        elif gate == "max_penalty":
            if perfect_group:
                contributing = perfect_group
                cost = _metric_mean(contributing, metric)
                imputed = 0
            else:
                contributing = []
                cost = max_by_ft[(task_id, finder_id)]
                imputed = 1
        else:
            raise ValueError(gate)

        if cost <= 0:
            continue
        out[key] = {
            "task_id": task_id,
            "builder_id": builder_id,
            "finder_id": finder_id,
            "cost": cost,
            "metric": metric,
            "gate": gate,
            "imputed_penalty": imputed,
            "n_runs_total": len(group),
            "n_runs_contributing": len(contributing),
            "n_perfect_runs": len(perfect_group),
            "mean_accuracy_all_runs": _mean([row["accuracy"] for row in group]),
            "mean_accuracy_contributing": _mean([row["accuracy"] for row in contributing])
            if contributing
            else math.nan,
            "mean_original_accuracy_all_runs": _mean([row["original_accuracy"] for row in group]),
            "mean_question_count": _mean([row["question_count"] for row in group]),
        }
    return out


def _builder_score_rows(
    panel: Panel,
    gate: Gate,
    metric: str,
    means: dict[tuple[str, str, str], dict[str, Any]],
) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in means.values():
        grouped[(row["task_id"], row["finder_id"])].append(row)

    ratios: dict[str, list[float]] = defaultdict(list)
    accuracies: dict[str, list[float]] = defaultdict(list)
    original_accuracies: dict[str, list[float]] = defaultdict(list)
    question_counts: dict[str, list[float]] = defaultdict(list)
    penalties: dict[str, int] = defaultdict(int)
    for group_rows in grouped.values():
        min_cost = min(row["cost"] for row in group_rows)
        if min_cost <= 0:
            continue
        for row in group_rows:
            builder_id = row["builder_id"]
            ratios[builder_id].append(row["cost"] / min_cost)
            accuracies[builder_id].append(row["mean_accuracy_all_runs"])
            original_accuracies[builder_id].append(row["mean_original_accuracy_all_runs"])
            question_counts[builder_id].append(row["mean_question_count"])
            penalties[builder_id] += int(row["imputed_penalty"])

    finder_ids = {row["finder_id"] for row in means.values()}
    task_ids = {row["task_id"] for row in means.values()}
    max_cells = len(finder_ids) * len(task_ids)
    scored = []
    for builder_id, values in ratios.items():
        scored.append(
            {
                "panel_id": panel.panel_id,
                "primary_panel": int(panel.primary),
                "conditioned_on": "implemented_gold",
                "gate": gate,
                "metric": metric,
                "builder_id": builder_id,
                "family": _agent_family(builder_id),
                "base_model": _agent_base(builder_id),
                "effort": _agent_effort(builder_id),
                "score": _geomean(values),
                "n_cells": len(values),
                "max_cells": max_cells,
                "cell_coverage": _ratio(len(values), max_cells),
                "n_penalty_cells": penalties[builder_id],
                "mean_cell_accuracy": _mean(accuracies[builder_id]),
                "mean_original_cell_accuracy": _mean(original_accuracies[builder_id]),
                "mean_question_count": _mean(question_counts[builder_id]),
                "ci95_low": math.nan,
                "ci95_high": math.nan,
            }
        )
    scored.sort(key=lambda row: (row["score"], -row["n_cells"], row["builder_id"]))
    for rank, row in enumerate(scored, start=1):
        row["rank"] = rank
    return scored


def _rank_stability_rows(
    panel: Panel,
    gate: Gate,
    metric: str,
    means: dict[tuple[str, str, str], dict[str, Any]],
) -> list[dict[str, Any]]:
    ranks: dict[tuple[str, str], dict[str, int]] = {}
    costs: dict[tuple[str, str], dict[str, float]] = defaultdict(dict)
    for (task_id, builder_id, finder_id), row in means.items():
        costs[(task_id, finder_id)][builder_id] = row["cost"]
    for task_finder, builder_costs in costs.items():
        ordered = sorted(builder_costs.items(), key=lambda item: (item[1], item[0]))
        ranks[task_finder] = {
            builder_id: rank for rank, (builder_id, _cost) in enumerate(ordered, start=1)
        }

    rows: list[dict[str, Any]] = []
    task_ids = sorted({task_id for task_id, _finder_id in ranks})
    for task_id in task_ids:
        finder_ids = sorted(finder_id for task, finder_id in ranks if task == task_id)
        pair_rows = []
        for i, finder_a in enumerate(finder_ids):
            for finder_b in finder_ids[i + 1 :]:
                tau = _kendall_tau_a(ranks[(task_id, finder_a)], ranks[(task_id, finder_b)])
                if math.isnan(tau):
                    continue
                pair_rows.append(
                    {
                        "finder_a": finder_a,
                        "finder_b": finder_b,
                        "tau": tau,
                        "same_family": _agent_family(finder_a) == _agent_family(finder_b),
                    }
                )
        for slice_id, selected in (
            ("all_pairs", pair_rows),
            ("same_family_pairs", [row for row in pair_rows if row["same_family"]]),
            ("cross_family_pairs", [row for row in pair_rows if not row["same_family"]]),
        ):
            rows.append(
                {
                    "panel_id": panel.panel_id,
                    "primary_panel": int(panel.primary),
                    "conditioned_on": "implemented_gold",
                    "gate": gate,
                    "metric": metric,
                    "task_id": task_id,
                    "slice_id": slice_id,
                    "n_pairs": len(selected),
                    "median_tau": statistics.median([row["tau"] for row in selected])
                    if selected
                    else math.nan,
                    "mean_tau": _mean([row["tau"] for row in selected]) if selected else math.nan,
                    "positive_fraction": _mean([int(row["tau"] > 0) for row in selected])
                    if selected
                    else math.nan,
                }
            )
    return rows


def _kendall_tau_a(a_ranks: dict[str, int], b_ranks: dict[str, int]) -> float:
    common = sorted(set(a_ranks) & set(b_ranks))
    if len(common) < 2:
        return math.nan
    concordant = 0
    discordant = 0
    for i, x in enumerate(common):
        for y in common[i + 1 :]:
            da = a_ranks[x] - a_ranks[y]
            db = b_ranks[x] - b_ranks[y]
            if da * db > 0:
                concordant += 1
            elif da * db < 0:
                discordant += 1
    denom = concordant + discordant
    return (concordant - discordant) / denom if denom else math.nan


def _conditioned_builder_summary_rows(
    run_rows: list[dict[str, Any]],
    *,
    original_builder_summary: list[dict[str, str]],
    builder_conformance_rows: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    by_builder: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in run_rows:
        by_builder[row["builder_id"]].append(row)
    source_by_builder = {row["builder_id"]: row for row in original_builder_summary}
    coverage = _builder_coverage_by_id(builder_conformance_rows)

    rows: list[dict[str, Any]] = []
    for builder_id, group in sorted(by_builder.items()):
        source = source_by_builder.get(builder_id, {})
        coverage_row = coverage[builder_id]
        question_rows = sum(int(row["question_count"]) for row in group)
        correct = sum(float(row["accuracy"]) * int(row["question_count"]) for row in group)
        downstream_accuracy = _ratio(correct, question_rows)
        ci_low, ci_high = _normal_ci95([float(row["accuracy"]) for row in group])
        rows.append(
            {
                "builder_id": builder_id,
                "family": _agent_family(builder_id),
                "base_model": _agent_base(builder_id),
                "effort": _agent_effort(builder_id),
                "downstream_accuracy": downstream_accuracy,
                "artifact_present_pairs": coverage_row["artifact_present_pairs"],
                "artifact_present_implemented_gold": coverage_row["artifact_present_implemented_gold"],
                "artifact_present_implemented_gold_rate": coverage_row[
                    "artifact_present_implemented_gold_rate"
                ],
                "compile_pass_pairs": coverage_row["compile_pass_pairs"],
                "compile_pass_implemented_gold": coverage_row["compile_pass_implemented_gold"],
                "compile_pass_implemented_gold_rate": coverage_row[
                    "compile_pass_implemented_gold_rate"
                ],
                "implementation_adjusted_downstream_recovery": downstream_accuracy
                * coverage_row["artifact_present_implemented_gold_rate"],
                "downstream_run_ci95_low": ci_low,
                "downstream_run_ci95_high": ci_high,
                "median_build_tokens": source.get("median_build_tokens", ""),
                "mean_build_tokens": source.get("mean_build_tokens", ""),
                "total_build_tokens": source.get("total_build_tokens", ""),
                "total_build_cost_usd": source.get("total_build_cost_usd", ""),
                "build_runs": source.get("build_runs", ""),
                "question_rows": question_rows,
            }
        )
    rows.sort(key=lambda row: (-float(row["downstream_accuracy"]), row["builder_id"]))
    return rows


def _builder_coverage_by_id(rows: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    grouped: dict[str, dict[str, int]] = defaultdict(
        lambda: {
            "artifact_present_pairs": 0,
            "artifact_present_implemented_gold": 0,
            "compile_pass_pairs": 0,
            "compile_pass_implemented_gold": 0,
        }
    )
    for row in rows:
        builder_id = str(row["builder_id"])
        if builder_id == "all" or str(row["task_id"]) == "all":
            continue
        pairs = int(row["artifact_question_pairs"])
        implemented = int(row["implemented_gold"])
        grouped[builder_id]["artifact_present_pairs"] += pairs
        grouped[builder_id]["artifact_present_implemented_gold"] += implemented
        if str(row["compile_pass"]) == "true":
            grouped[builder_id]["compile_pass_pairs"] += pairs
            grouped[builder_id]["compile_pass_implemented_gold"] += implemented

    out: dict[str, dict[str, Any]] = {}
    for builder_id, counts in grouped.items():
        out[builder_id] = {
            **counts,
            "artifact_present_implemented_gold_rate": _ratio(
                counts["artifact_present_implemented_gold"],
                counts["artifact_present_pairs"],
            ),
            "compile_pass_implemented_gold_rate": _ratio(
                counts["compile_pass_implemented_gold"],
                counts["compile_pass_pairs"],
            ),
        }
    return out


def _normal_ci95(values: list[float]) -> tuple[float, float]:
    clean = [value for value in values if not math.isnan(value)]
    if not clean:
        return math.nan, math.nan
    mean = _mean(clean)
    if len(clean) < 2:
        return mean, mean
    se = statistics.stdev(clean) / math.sqrt(len(clean))
    return max(0.0, mean - 1.96 * se), min(1.0, mean + 1.96 * se)


def _conditioned_agent_view_rows(
    answer_rows: list[dict[str, Any]],
    *,
    implementation_signal_questions: set[tuple[str, str]],
    low_prior_questions: set[tuple[str, str]],
) -> list[dict[str, Any]]:
    top4_builders = _top_agents(answer_rows, role_key="builder_id", limit=4)
    top6_builders = _top_agents(answer_rows, role_key="builder_id", limit=6)
    top4_finders = _top_agents(answer_rows, role_key="finder_id", limit=4)
    top6_finders = _top_agents(answer_rows, role_key="finder_id", limit=6)
    views = [
        (
            "builder",
            "all_finders",
            "All finders judging each builder",
            answer_rows,
        ),
        (
            "builder",
            "top4_formal_finders",
            "Only the top four formal-accuracy finders judge builders",
            [row for row in answer_rows if row["finder_id"] in top4_finders],
        ),
        (
            "builder",
            "top6_formal_finders",
            "Only the top six formal-accuracy finders judge builders",
            [row for row in answer_rows if row["finder_id"] in top6_finders],
        ),
        (
            "builder",
            "cross_family_finders",
            "Only cross-family finder judgments",
            [row for row in answer_rows if row["builder_family"] != row["finder_family"]],
        ),
        (
            "builder",
            "implementation_signal_questions",
            "Questions where formal agreement exceeds question-only control",
            [
                row
                for row in answer_rows
                if (row["task_id"], row["question_id"]) in implementation_signal_questions
            ],
        ),
        (
            "builder",
            "low_prior_questions",
            "Questions with question-only three-trial agreement < 0.90",
            [row for row in answer_rows if (row["task_id"], row["question_id"]) in low_prior_questions],
        ),
        (
            "finder",
            "all_builders",
            "All builders judged by each finder",
            answer_rows,
        ),
        (
            "finder",
            "top4_builders",
            "Finder quality on top four builders",
            [row for row in answer_rows if row["builder_id"] in top4_builders],
        ),
        (
            "finder",
            "top6_builders",
            "Finder quality on top six builders",
            [row for row in answer_rows if row["builder_id"] in top6_builders],
        ),
        (
            "finder",
            "cross_family_builders",
            "Only cross-family builder inputs",
            [row for row in answer_rows if row["builder_family"] != row["finder_family"]],
        ),
        (
            "finder",
            "implementation_signal_questions",
            "Questions where formal agreement exceeds question-only control",
            [
                row
                for row in answer_rows
                if (row["task_id"], row["question_id"]) in implementation_signal_questions
            ],
        ),
        (
            "finder",
            "low_prior_questions",
            "Questions with question-only three-trial agreement < 0.90",
            [row for row in answer_rows if (row["task_id"], row["question_id"]) in low_prior_questions],
        ),
    ]

    out: list[dict[str, Any]] = []
    role_key = {"builder": "builder_id", "finder": "finder_id"}
    for role, view_id, description, rows in views:
        grouped: dict[str, list[int]] = defaultdict(list)
        for row in rows:
            grouped[row[role_key[role]]].append(int(row["is_correct"]))
        ranked = sorted(grouped.items(), key=lambda item: (_mean(item[1]), item[0]), reverse=True)
        for rank, (agent_id, values) in enumerate(ranked, start=1):
            out.append(
                {
                    "role": role,
                    "view_id": view_id,
                    "description": description,
                    "rank": rank,
                    "agent_id": agent_id,
                    "family": _agent_family(agent_id),
                    "base_model": _agent_base(agent_id),
                    "effort": _agent_effort(agent_id),
                    "accuracy": _mean(values),
                    "question_rows": len(values),
                }
            )
    return out


def _top_agents(rows: list[dict[str, Any]], *, role_key: str, limit: int) -> list[str]:
    grouped: dict[str, list[int]] = defaultdict(list)
    for row in rows:
        grouped[row[role_key]].append(int(row["is_correct"]))
    ranked = sorted(grouped.items(), key=lambda item: (_mean(item[1]), item[0]), reverse=True)
    return [agent_id for agent_id, _values in ranked[:limit]]


def _affinity_residual_rows(run_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str, str], list[dict[str, float]]] = defaultdict(list)
    for row in run_rows:
        grouped[(row["task_id"], row["builder_id"], row["finder_id"])].append(
            {
                "accuracy": float(row["accuracy"]),
                "retrieval_bytes": float(row["retrieval_bytes"]),
                "question_count": float(row["question_count"]),
            }
        )

    task_cell_scores: list[dict[str, Any]] = []
    for (task_id, builder_id, finder_id), group in grouped.items():
        effort = _mean([row["retrieval_bytes"] for row in group])
        if effort <= 0:
            continue
        mean_accuracy = _mean([row["accuracy"] for row in group])
        question_count = int(round(_mean([row["question_count"] for row in group])))
        success_signal = mean_accuracy**question_count
        task_cell_scores.append(
            {
                "task_id": task_id,
                "builder_id": builder_id,
                "finder_id": finder_id,
                "raw_recovery": success_signal / effort,
                "mean_accuracy": mean_accuracy,
                "success_signal": success_signal,
                "mean_retrieval_bytes": effort,
            }
        )

    best_by_task_finder_block: dict[tuple[str, str, str], float] = defaultdict(float)
    for row in task_cell_scores:
        key = (row["task_id"], row["finder_id"], _agent_effort(row["builder_id"]))
        best_by_task_finder_block[key] = max(best_by_task_finder_block[key], row["raw_recovery"])

    by_builder_finder: dict[tuple[str, str], list[dict[str, float]]] = defaultdict(list)
    for row in task_cell_scores:
        normalization_group = (row["task_id"], row["finder_id"], _agent_effort(row["builder_id"]))
        best = best_by_task_finder_block[normalization_group]
        if best <= 0:
            continue
        by_builder_finder[(row["builder_id"], row["finder_id"])].append(
            {
                "normalized_score": max(row["raw_recovery"] / best, 1e-9),
                "mean_accuracy": row["mean_accuracy"],
                "success_signal": row["success_signal"],
                "mean_retrieval_bytes": row["mean_retrieval_bytes"],
            }
        )

    rows: list[dict[str, Any]] = []
    for builder_id in AGENT_ORDER:
        for finder_id in AGENT_ORDER:
            cells = by_builder_finder[(builder_id, finder_id)]
            score = _geomean([cell["normalized_score"] for cell in cells]) if cells else math.nan
            rows.append(
                {
                    "builder_id": builder_id,
                    "builder_label": _agent_display_label(builder_id),
                    "finder_id": finder_id,
                    "finder_label": _agent_display_label(finder_id),
                    "score": score,
                    "normalization_scope": "within_builder_effort_block_per_task_finder",
                    "task_cells": len(cells),
                    "mean_accuracy": _mean([cell["mean_accuracy"] for cell in cells])
                    if cells
                    else math.nan,
                    "mean_success_signal": _mean([cell["success_signal"] for cell in cells])
                    if cells
                    else math.nan,
                    "mean_retrieval_bytes": _mean([cell["mean_retrieval_bytes"] for cell in cells])
                    if cells
                    else math.nan,
                    "builder_family": _family_label(builder_id),
                    "finder_family": _family_label(finder_id),
                    "same_family": _family_label(builder_id) == _family_label(finder_id),
                    "same_base_model": _agent_base(builder_id) == _agent_base(finder_id),
                }
            )

    scores = [float(row["score"]) for row in rows if not math.isnan(float(row["score"]))]
    grand_mean = _mean(scores)
    builder_means = {
        builder_id: _mean(
            [
                float(row["score"])
                for row in rows
                if row["builder_id"] == builder_id and not math.isnan(float(row["score"]))
            ]
        )
        for builder_id in AGENT_ORDER
    }
    finder_means = {
        finder_id: _mean(
            [
                float(row["score"])
                for row in rows
                if row["finder_id"] == finder_id and not math.isnan(float(row["score"]))
            ]
        )
        for finder_id in AGENT_ORDER
    }
    for row in rows:
        score = float(row["score"])
        if math.isnan(score):
            row["builder_mean"] = math.nan
            row["finder_mean"] = math.nan
            row["grand_mean"] = grand_mean
            row["affinity_residual"] = math.nan
            continue
        row["builder_mean"] = builder_means[row["builder_id"]]
        row["finder_mean"] = finder_means[row["finder_id"]]
        row["grand_mean"] = grand_mean
        row["affinity_residual"] = score - row["builder_mean"] - row["finder_mean"] + grand_mean
    return rows


def _family_residual_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    families = ["OpenAI/Codex", "Anthropic/Claude", "MiMo"]
    out: list[dict[str, Any]] = []
    for builder_family in families:
        row_out: dict[str, Any] = {"builder_family": builder_family}
        for finder_family in families:
            values = [
                float(row["affinity_residual"])
                for row in rows
                if row["builder_family"] == builder_family
                and row["finder_family"] == finder_family
                and not math.isnan(float(row["affinity_residual"]))
            ]
            row_out[finder_family] = _mean(values)
        out.append(row_out)
    return out


def _family_label(agent_id: str) -> str:
    family = _agent_family(agent_id)
    if family == "codex":
        return "OpenAI/Codex"
    if family == "claude":
        return "Anthropic/Claude"
    if family == "mimo":
        return "MiMo"
    return family


def _agent_display_label(agent_id: str) -> str:
    labels = {
        "claude_code_opus_4_7_high": "Opus 4.7",
        "claude_code_sonnet_4_6_high": "Sonnet 4.6",
        "codex_gpt5_4_mini_high": "GPT-5.4 Mini",
        "codex_gpt5_5_high": "GPT-5.5",
        "claude_code_opus_4_7_low": "Opus 4.7-low",
        "claude_code_sonnet_4_6_low": "Sonnet 4.6-low",
        "codex_gpt5_4_mini_low": "GPT-5.4 Mini-low",
        "codex_gpt5_5_low": "GPT-5.5-low",
        "claude_code_mimo_v2_5_pro_high": "MiMo 2.5 Pro",
        "claude_code_mimo_v2_5_pro_low": "MiMo 2.5 Pro-low",
        "claude_code_mimo_v2_5_high": "MiMo 2.5",
        "claude_code_mimo_v2_5_low": "MiMo 2.5-low",
    }
    return labels.get(agent_id, agent_id)


def _add_builder_score_bootstrap_cis(
    builder_score_rows: list[dict[str, Any]],
    panels: list[Panel],
    run_rows: list[dict[str, Any]],
) -> None:
    by_key = {
        (row["panel_id"], row["gate"], row["metric"], row["builder_id"]): row
        for row in builder_score_rows
    }
    for panel in panels:
        if not panel.primary:
            continue
        panel_rows = _panel_run_rows(run_rows, panel)
        intervals = _bootstrap_builder_scores(
            panel,
            panel_rows,
            gate="perfect_run",
            metric="total_bytes",
        )
        for builder_id, (lo, hi) in intervals.items():
            row = by_key.get((panel.panel_id, "perfect_run", "total_bytes", builder_id))
            if row is None:
                continue
            row["ci95_low"] = lo
            row["ci95_high"] = hi


def _bootstrap_builder_scores(
    panel: Panel,
    rows: list[dict[str, Any]],
    *,
    gate: Gate,
    metric: str,
) -> dict[str, tuple[float, float]]:
    rng = random.Random(BOOTSTRAP_SEED)
    clusters: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        clusters[(row["finder_id"], row["task_id"])].append(row)
    cluster_keys = sorted(clusters)
    values: dict[str, list[float]] = defaultdict(list)
    for _ in range(BOOTSTRAP_REPS):
        sampled: list[dict[str, Any]] = []
        for _cluster in cluster_keys:
            sampled.extend(clusters[rng.choice(cluster_keys)])
        means = _cell_means(sampled, metric=metric, gate=gate)
        for score_row in _builder_score_rows(panel, gate, metric, means):
            values[score_row["builder_id"]].append(score_row["score"])
    return {
        builder_id: (_percentile(samples, 0.025), _percentile(samples, 0.975))
        for builder_id, samples in values.items()
        if samples
    }


def _percentile(values: list[float], q: float) -> float:
    ordered = sorted(values)
    if not ordered:
        return math.nan
    index = min(len(ordered) - 1, max(0, int(q * len(ordered))))
    return ordered[index]


def _recovery_rows(panel: Panel, rows: list[dict[str, Any]], effort_metric: str) -> list[dict[str, Any]]:
    grouped: dict[tuple[str, str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        grouped[(row["task_id"], row["builder_id"], row["finder_id"])].append(row)

    cell_scores: list[dict[str, Any]] = []
    for (task_id, builder_id, finder_id), group in grouped.items():
        effort = _metric_mean(group, effort_metric)
        if effort <= 0:
            continue
        mean_accuracy = _mean([row["accuracy"] for row in group])
        mean_original_accuracy = _mean([row["original_accuracy"] for row in group])
        question_count = int(round(_mean([row["question_count"] for row in group])))
        original_question_count = int(round(_mean([row["original_question_count"] for row in group])))
        perfect_run_rate = _mean([row["perfect_run"] for row in group])
        cell_scores.append(
            {
                "task_id": task_id,
                "builder_id": builder_id,
                "finder_id": finder_id,
                "mean_accuracy": mean_accuracy,
                "mean_original_accuracy": mean_original_accuracy,
                "perfect_run_rate": perfect_run_rate,
                "question_count": question_count,
                "original_question_count": original_question_count,
                "mean_effort": effort,
            }
        )

    accuracies: dict[str, list[float]] = defaultdict(list)
    original_accuracies: dict[str, list[float]] = defaultdict(list)
    efforts: dict[str, list[float]] = defaultdict(list)
    perfect_rates: dict[str, list[float]] = defaultdict(list)
    question_counts: dict[str, list[float]] = defaultdict(list)
    original_question_counts: dict[str, list[float]] = defaultdict(list)
    zero_accuracy_cells: dict[str, int] = defaultdict(int)
    for row in cell_scores:
        builder_id = row["builder_id"]
        accuracies[builder_id].append(row["mean_accuracy"])
        original_accuracies[builder_id].append(row["mean_original_accuracy"])
        efforts[builder_id].append(row["mean_effort"])
        perfect_rates[builder_id].append(row["perfect_run_rate"])
        question_counts[builder_id].append(row["question_count"])
        original_question_counts[builder_id].append(row["original_question_count"])
        if row["mean_accuracy"] <= 0:
            zero_accuracy_cells[builder_id] += 1

    finder_ids = {row["finder_id"] for row in rows}
    task_ids = {row["task_id"] for row in rows}
    max_cells = len(finder_ids) * len(task_ids)
    out = []
    for builder_id, values in accuracies.items():
        out.append(
            {
                "panel_id": panel.panel_id,
                "primary_panel": int(panel.primary),
                "conditioned_on": "implemented_gold",
                "effort_metric": effort_metric,
                "recovery_variant": FIGURE1_RECOVERY_VARIANT,
                "primary_recovery": 1,
                "builder_id": builder_id,
                "family": _agent_family(builder_id),
                "base_model": _agent_base(builder_id),
                "effort": _agent_effort(builder_id),
                "score": _mean(values),
                "n_cells": len(values),
                "max_cells": max_cells,
                "cell_coverage": _ratio(len(values), max_cells),
                "mean_cell_accuracy": _mean(accuracies[builder_id]),
                "mean_original_cell_accuracy": _mean(original_accuracies[builder_id]),
                "mean_perfect_run_rate": _mean(perfect_rates[builder_id]),
                "mean_cell_effort": _mean(efforts[builder_id]),
                "mean_question_count": _mean(question_counts[builder_id]),
                "mean_original_question_count": _mean(original_question_counts[builder_id]),
                "n_zero_accuracy_cells": zero_accuracy_cells[builder_id],
            }
        )
    out.sort(key=lambda row: (-row["score"], row["builder_id"]))
    for rank, row in enumerate(out, start=1):
        row["rank"] = rank
    return out


def _score_comparison_rows(
    *,
    original_recovery_path: Path,
    original_builder_path: Path,
    conditioned_recovery_rows: list[dict[str, Any]],
    conditioned_builder_rows: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    original_recovery = _read_csv(original_recovery_path)
    original_builder = _read_csv(original_builder_path)
    comparison: list[dict[str, Any]] = []

    conditioned_recovery_by_key = {
        (row["panel_id"], row["builder_id"], row["effort_metric"], row["recovery_variant"]): row
        for row in conditioned_recovery_rows
    }
    for original in original_recovery:
        if (
            original["panel_id"] not in {"high_effort", "low_effort"}
            or original["effort_metric"] != FIGURE1_EFFORT_METRIC
            or original["recovery_variant"] != ORIGINAL_FIGURE1_RECOVERY_VARIANT
        ):
            continue
        conditioned = conditioned_recovery_by_key.get(
            (
                original["panel_id"],
                original["builder_id"],
                FIGURE1_EFFORT_METRIC,
                FIGURE1_RECOVERY_VARIANT,
            )
        )
        if conditioned is None:
            continue
        comparison.append(
            _comparison_row(
                figure_id="figure1_recovery",
                panel_id=original["panel_id"],
                builder_id=original["builder_id"],
                higher_is_better=True,
                original=original,
                conditioned=conditioned,
            )
        )

    conditioned_builder_by_key = {
        (row["panel_id"], row["builder_id"], row["gate"], row["metric"]): row
        for row in conditioned_builder_rows
    }
    for original in original_builder:
        if (
            original["panel_id"] not in {"high_effort", "low_effort"}
            or original["gate"] != PRIMARY_FIGURE2_KEY[0]
            or original["metric"] != PRIMARY_FIGURE2_KEY[1]
        ):
            continue
        conditioned = conditioned_builder_by_key.get(
            (original["panel_id"], original["builder_id"], original["gate"], original["metric"])
        )
        if conditioned is None:
            continue
        comparison.append(
            _comparison_row(
                figure_id="figure2_builder_rb",
                panel_id=original["panel_id"],
                builder_id=original["builder_id"],
                higher_is_better=False,
                original=original,
                conditioned=conditioned,
            )
        )
    grouped: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in comparison:
        grouped[(row["figure_id"], row["panel_id"])].append(row)
    for rows in grouped.values():
        higher_is_better = bool(rows[0]["higher_is_better"])
        rows.sort(key=lambda row: row["conditioned_score"], reverse=higher_is_better)
        for rank, row in enumerate(rows, start=1):
            row["conditioned_rank"] = rank
        rows.sort(key=lambda row: row["original_score"], reverse=higher_is_better)
        for rank, row in enumerate(rows, start=1):
            row["original_rank"] = rank
    comparison.sort(key=lambda row: (row["figure_id"], row["panel_id"], row["conditioned_rank"]))
    return comparison


def _comparison_row(
    *,
    figure_id: str,
    panel_id: str,
    builder_id: str,
    higher_is_better: bool,
    original: dict[str, Any],
    conditioned: dict[str, Any],
) -> dict[str, Any]:
    if figure_id == "figure1_recovery":
        original_score = _as_float(str(original["mean_cell_accuracy"]))
        conditioned_score = float(conditioned["mean_cell_accuracy"])
    else:
        original_score = _as_float(str(original["score"]))
        conditioned_score = float(conditioned["score"])
    original_rank = int(original["rank"])
    conditioned_rank = int(conditioned["rank"])
    return {
        "figure_id": figure_id,
        "panel_id": panel_id,
        "builder_id": builder_id,
        "higher_is_better": int(higher_is_better),
        "original_rank": original_rank,
        "conditioned_rank": conditioned_rank,
        "rank_delta": conditioned_rank - original_rank,
        "original_score": original_score,
        "conditioned_score": conditioned_score,
        "score_delta": conditioned_score - original_score,
        "score_ratio": conditioned_score / original_score if original_score else math.nan,
        "original_n_cells": int(original["n_cells"]),
        "conditioned_n_cells": int(conditioned["n_cells"]),
        "conditioned_max_cells": int(conditioned["max_cells"]),
        "original_cell_coverage": _as_float(str(original["cell_coverage"])),
        "conditioned_cell_coverage": float(conditioned["cell_coverage"]),
        "original_mean_cell_accuracy": _as_float(str(original["mean_cell_accuracy"])),
        "conditioned_mean_cell_accuracy": float(conditioned["mean_cell_accuracy"]),
        "conditioned_mean_question_count": float(conditioned.get("mean_question_count", math.nan)),
    }


def _metric_mean(rows: list[dict[str, Any]], metric: str) -> float:
    values = [float(row[metric]) for row in rows if row[metric] is not None]
    return _mean(values)


def _geomean(values: list[float]) -> float:
    positive = [value for value in values if value > 0 and not math.isnan(value)]
    if not positive:
        return math.nan
    return math.exp(sum(math.log(value) for value in positive) / len(positive))


def _ratio(numerator: int | float, denominator: int | float) -> float:
    if denominator == 0:
        return math.nan
    return numerator / denominator


def _fmt(value: Any, digits: int = 3) -> str:
    if value in {"", None}:
        return ""
    numeric = float(value)
    if math.isnan(numeric):
        return ""
    return f"{numeric:.{digits}f}"


def _fmt_pct(value: Any, digits: int = 1) -> str:
    if value in {"", None}:
        return "--"
    numeric = float(value)
    if math.isnan(numeric):
        return "--"
    return f"{numeric * 100:.{digits}f}%"


def _write_markdown(
    path: Path,
    *,
    manifest: dict[str, Any],
    gate_summary_rows: list[dict[str, Any]],
    accuracy_label_rows: list[dict[str, Any]],
    builder_conformance_rows: list[dict[str, Any]],
    comparison_rows: list[dict[str, Any]],
) -> None:
    gate_by_key = {(row["scope_id"], row["task_id"]): row for row in gate_summary_rows}
    comparison_by = defaultdict(list)
    for row in comparison_rows:
        comparison_by[(row["figure_id"], row["panel_id"])].append(row)

    lines = [
        "# Artifact Conformance Impact Analysis",
        "",
        f"Generated: {manifest['generated_at']}",
        "",
        "This report conditions finder metrics on artifact-question pairs labeled",
        "`implemented_gold`. Other labels are counted as artifact conformance failures",
        "or ambiguous implementation evidence, not as recoverable gold behavior.",
        "",
        "## Gate Summary",
        "",
        "| scope | task | builds | pairs | gold | non-gold | absent/ambig | excluded | original acc | gold-only acc | excluded acc |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for scope_id in ("artifact_present_all", "compile_pass_primary", "compile_failed_diagnostic"):
        for task_id in ("scratch_minidb", "scratch_nanoweb", "all"):
            row = gate_by_key.get((scope_id, task_id))
            if row is None:
                continue
            lines.append(
                f"| {scope_id} | {task_id} | {row['build_runs']} | {row['artifact_question_pairs']} | "
                f"{row['implemented_gold']} ({_fmt_pct(row['implemented_gold_rate'])}) | "
                f"{row['implemented_non_gold']} | {row['absent_or_ambiguous']} | "
                f"{row['excluded_pairs']} ({_fmt_pct(row['excluded_pair_rate'])}) | "
                f"{_fmt_pct(row['accuracy_original'])} | {_fmt_pct(row['accuracy_implemented_gold'])} | "
                f"{_fmt_pct(row['accuracy_excluded'])} |"
            )

    lines.extend(
        [
            "",
            "## Primary Figure 1 Sensitivity",
            "",
            "Higher is better. The plotted gate is simple mean audited answer recovery "
            "over contributing finder-task cells; the cells column reports coverage separately.",
            "",
        ]
    )
    _append_comparison_table(lines, comparison_by, "figure1_recovery")

    lines.extend(
        [
            "",
            "## Primary Figure 2 Sensitivity",
            "",
            "Lower is better. Scores are within-finder-task total-byte ratios on conditioned all-correct cells.",
            "",
        ]
    )
    _append_comparison_table(lines, comparison_by, "figure2_builder_rb")

    high_risk_builders = [
        row
        for row in builder_conformance_rows
        if row["builder_id"] != "all" and row["excluded_pairs"] > 0
    ]
    high_risk_builders = sorted(
        high_risk_builders,
        key=lambda row: (
            -row["excluded_pair_rate"],
            -row["excluded_pairs"],
            row["task_id"],
            row["compile_pass"],
            row["builder_id"],
        ),
    )[:12]
    lines.extend(
        [
            "",
            "## Highest Exclusion Builders",
            "",
            "| task | compile | builder | pairs | gold rate | excluded |",
            "| --- | --- | --- | ---: | ---: | ---: |",
        ]
    )
    for row in high_risk_builders:
        lines.append(
            f"| {row['task_id']} | {row['compile_pass']} | {row['builder_id']} | "
            f"{row['artifact_question_pairs']} | {_fmt_pct(row['implemented_gold_rate'])} | "
            f"{row['excluded_pairs']} |"
        )

    label_rows = [
        row
        for row in accuracy_label_rows
        if row["scope_id"] == "compile_pass_primary" and row["task_id"] == "all"
    ]
    lines.extend(
        [
            "",
            "## Primary Accuracy By Label",
            "",
            "| label | pairs | answer rows | accuracy |",
            "| --- | ---: | ---: | ---: |",
        ]
    )
    for row in label_rows:
        lines.append(
            f"| {row['final_label']} | {row['artifact_question_pairs']} | "
            f"{row['answer_rows']} | {_fmt_pct(row['accuracy'])} |"
        )

    lines.extend(
        [
            "",
            "## Files",
            "",
            "- `conformance_gate_summary.csv`",
            "- `accuracy_by_conformance_label.csv`",
            "- `builder_conformance_summary.csv`",
            "- `conditioned_run_rows.csv`",
            "- `conditioned_recovery_scores.csv`",
            "- `conditioned_builder_scores.csv`",
            "- `score_comparison.csv`",
            "- `manifest.json`",
        ]
    )
    path.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")


def _append_comparison_table(
    lines: list[str],
    comparison_by: dict[tuple[str, str], list[dict[str, Any]]],
    figure_id: str,
) -> None:
    for panel_id in ("high_effort", "low_effort"):
        rows = comparison_by[(figure_id, panel_id)]
        lines.extend(
            [
                f"### {panel_id}",
                "",
                "| conditioned rank | builder | original rank | original score | conditioned score | cells | mean acc | mean q |",
                "| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |",
            ]
        )
        for row in rows:
            lines.append(
                f"| {row['conditioned_rank']} | {row['builder_id']} | {row['original_rank']} | "
                f"{_fmt(row['original_score'])} | {_fmt(row['conditioned_score'])} | "
                f"{row['conditioned_n_cells']}/{row['conditioned_max_cells']} | "
                f"{_fmt(row['conditioned_mean_cell_accuracy'])} | "
                f"{_fmt(row['conditioned_mean_question_count'], 1)} |"
            )
        lines.append("")


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.resolve().as_posix()


if __name__ == "__main__":
    main()
