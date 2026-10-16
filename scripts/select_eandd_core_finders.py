"""Select a fixed core finder panel from frozen E&D calibration outputs.

The selector is intentionally conservative. Formal artifact-conditioned
accuracy alone is not enough for a future benchmark instrument because a finder
can score highly from task priors. The default core gate therefore requires
high formal recovery, positive finder-specific question-only lift on the main
and diagnostic slices, low cost, low variance, non-extreme affinity, and
agreement with other finders' builder-effort ordering.
"""

# ruff: noqa: E402

from __future__ import annotations

import csv
import hashlib
import json
import math
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from bnf.config import paths


CORE_FORMAL_CI_LOW_MIN = 0.970
CORE_FORMAL_CI_WIDTH_MAX = 0.015
CORE_MEAN_TOKENS_MAX = 150_000
CORE_RANK_STABILITY_MEAN_TAU_MIN = 0.40
AFFINITY_ABS_MAX = 0.05

AUDIT_FORMAL_ACCURACY_MIN = 0.965
AUDIT_LIFT_MIN = 0.10
AUDIT_MEAN_TOKENS_MAX = 500_000
AUDIT_RANK_STABILITY_MEAN_TAU_MIN = 0.40


def main() -> None:
    p = paths()
    report_dir = p.reports_dir / "eandd_final"
    out_dir = report_dir / "core_finder_selection"
    out_dir.mkdir(parents=True, exist_ok=True)

    pivot_manifest_path = report_dir / "pivots" / "manifest.json"
    pivot_manifest = json.loads(pivot_manifest_path.read_text(encoding="utf-8"))
    low_prior_questions = _question_set(pivot_manifest["low_prior_questions"])
    implementation_signal_questions = _question_set(pivot_manifest["implementation_signal_questions"])

    finder_rows = _load_finder_summary(report_dir / "finder_summary.csv")
    slice_metrics = _finder_slice_metrics(
        p.tables_dir / "question_answers.csv",
        low_prior_questions=low_prior_questions,
        implementation_signal_questions=implementation_signal_questions,
    )
    affinity = _load_affinity(report_dir / "pivots" / "affinity_by_agent.csv")
    rank_stability = _finder_rank_stability(
        report_dir / "within_finder_efficiency" / "within_finder_cells.csv"
    )

    rows = []
    for finder_id, summary in sorted(finder_rows.items()):
        row = {
            **summary,
            **slice_metrics[finder_id],
            **affinity[finder_id],
            **rank_stability[finder_id],
        }
        row["formal_ci_width"] = row["formal_run_ci95_high"] - row["formal_run_ci95_low"]
        row["core_gate_failures"] = _core_gate_failures(row)
        row["core_selected"] = not row["core_gate_failures"]
        row["audit_gate_failures"] = _audit_gate_failures(row)
        row["audit_candidate"] = (not row["core_selected"]) and not row["audit_gate_failures"]
        rows.append(row)

    core_rows = sorted(
        [row for row in rows if row["core_selected"]],
        key=lambda row: (-row["formal_accuracy"], row["mean_find_tokens"], row["finder_id"]),
    )
    core_finders = [row["finder_id"] for row in core_rows]
    audit_finders = _choose_audit_finders(rows)
    for row in rows:
        if row["finder_id"] in core_finders:
            row["selection"] = "core_default"
            row["selection_reason"] = "passes all default core gates"
        elif row["finder_id"] in audit_finders:
            row["selection"] = "prior_stress_audit"
            row["selection_reason"] = (
                "strong finder-specific prior lift; calibration audits only"
            )
        else:
            row["selection"] = "not_selected"
            row["selection_reason"] = "; ".join(row["core_gate_failures"])

    fieldnames = [
        "selection",
        "finder_id",
        "family",
        "base_model",
        "effort",
        "formal_accuracy",
        "formal_run_ci95_low",
        "formal_run_ci95_high",
        "formal_ci_width",
        "question_only_accuracy",
        "formal_minus_question_only",
        "low_prior_formal_accuracy",
        "low_prior_question_only_accuracy",
        "low_prior_lift",
        "implementation_signal_formal_accuracy",
        "implementation_signal_question_only_accuracy",
        "implementation_signal_lift",
        "spec_only_accuracy",
        "mean_find_tokens",
        "median_find_tokens",
        "rank_stability_mean_tau",
        "rank_stability_median_tau",
        "same_family_minus_different_family",
        "same_base_minus_different_base",
        "selection_reason",
    ]
    rows = _sort_selection_rows(rows)
    _write_csv(out_dir / "finder_selection.csv", fieldnames, rows)

    input_paths = [
        report_dir / "finder_summary.csv",
        report_dir / "pivots" / "manifest.json",
        report_dir / "pivots" / "affinity_by_agent.csv",
        report_dir / "within_finder_efficiency" / "within_finder_cells.csv",
        p.tables_dir / "question_answers.csv",
        p.tables_dir / "MANIFEST.json",
    ]
    table_manifest_path = p.tables_dir / "MANIFEST.json"
    table_manifest = (
        json.loads(table_manifest_path.read_text(encoding="utf-8"))
        if table_manifest_path.exists()
        else {}
    )
    manifest = {
        "selection_id": "eandd_core_finders_v1",
        "generated_at": table_manifest.get("generated_at") or pivot_manifest.get("generated_at"),
        "analysis_script": "scripts/select_eandd_core_finders.py",
        "default_core_finders": core_finders,
        "prior_stress_audit_finders": audit_finders,
        "rules": _rules_manifest(),
        "input_paths": [_display_path(path) for path in input_paths],
        "input_hashes": {
            _display_path(path): _sha256(path) for path in input_paths if path.exists()
        },
        "notes": [
            "Spec-only controls are saturated in this task pack and are recorded as validity context, not used as a hard inclusion gate.",
            "Routine future panels should use default_core_finders; prior_stress_audit_finders are optional calibration checks.",
        ],
    }
    (out_dir / "core_finders.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    _write_markdown(out_dir / "core_finders.md", manifest=manifest, rows=rows)
    print(f"wrote {out_dir / 'core_finders.json'}")


def _rules_manifest() -> dict[str, Any]:
    return {
        "default_core": {
            "formal_run_ci95_low_min": CORE_FORMAL_CI_LOW_MIN,
            "formal_ci_width_max": CORE_FORMAL_CI_WIDTH_MAX,
            "mean_find_tokens_max": CORE_MEAN_TOKENS_MAX,
            "formal_minus_question_only_min_exclusive": 0.0,
            "low_prior_lift_min_exclusive": 0.0,
            "implementation_signal_lift_min_exclusive": 0.0,
            "rank_stability_mean_tau_min": CORE_RANK_STABILITY_MEAN_TAU_MIN,
            "abs_same_family_minus_different_family_max": AFFINITY_ABS_MAX,
            "abs_same_base_minus_different_base_max": AFFINITY_ABS_MAX,
        },
        "prior_stress_audit": {
            "formal_accuracy_min": AUDIT_FORMAL_ACCURACY_MIN,
            "low_prior_lift_min": AUDIT_LIFT_MIN,
            "implementation_signal_lift_min": AUDIT_LIFT_MIN,
            "mean_find_tokens_max": AUDIT_MEAN_TOKENS_MAX,
            "rank_stability_mean_tau_min": AUDIT_RANK_STABILITY_MEAN_TAU_MIN,
            "abs_same_family_minus_different_family_max": AFFINITY_ABS_MAX,
            "abs_same_base_minus_different_base_max": AFFINITY_ABS_MAX,
            "selection_policy": "pick the audit candidate with the largest min(low_prior_lift, implementation_signal_lift)",
        },
    }


def _question_set(values: list[str]) -> set[tuple[str, str]]:
    return {tuple(value.split(":", 1)) for value in values}


def _load_finder_summary(path: Path) -> dict[str, dict[str, Any]]:
    rows = {}
    for row in _read_csv(path):
        finder_id = row["finder_id"]
        rows[finder_id] = {
            "finder_id": finder_id,
            "family": row["family"],
            "base_model": row["base_model"],
            "effort": row["effort"],
            "formal_accuracy": _float(row["formal_accuracy"]),
            "formal_run_ci95_low": _float(row["formal_run_ci95_low"]),
            "formal_run_ci95_high": _float(row["formal_run_ci95_high"]),
            "question_only_accuracy": _float(row["question_only_accuracy"]),
            "formal_minus_question_only": _float(row["formal_minus_question_only"]),
            "spec_only_accuracy": _float(row["spec_only_accuracy"]),
            "median_find_tokens": _float(row["median_find_tokens"]),
            "mean_find_tokens": _float(row["mean_find_tokens"]),
        }
    return rows


def _finder_slice_metrics(
    question_answers_path: Path,
    *,
    low_prior_questions: set[tuple[str, str]],
    implementation_signal_questions: set[tuple[str, str]],
) -> defaultdict[str, dict[str, float]]:
    counts: dict[tuple[str, str], list[int]] = defaultdict(lambda: [0, 0])
    for row in _read_csv(question_answers_path):
        finder_id = row["finder_id"]
        panels = set(row["panel_ids"].split("|"))
        task_question = (row["task_id"], row["question_id"])
        correct = 1 if row["is_correct"].lower() == "true" else 0

        if "paper_final_scratch" in panels and not row["builder_id"].startswith("control_"):
            _add(counts[(finder_id, "formal_all")], correct)
            if task_question in low_prior_questions:
                _add(counts[(finder_id, "formal_low_prior")], correct)
            if task_question in implementation_signal_questions:
                _add(counts[(finder_id, "formal_implementation_signal")], correct)
        if "question_only" in panels:
            _add(counts[(finder_id, "question_only_all")], correct)
            if task_question in low_prior_questions:
                _add(counts[(finder_id, "question_only_low_prior")], correct)
            if task_question in implementation_signal_questions:
                _add(counts[(finder_id, "question_only_implementation_signal")], correct)

    out: defaultdict[str, dict[str, float]] = defaultdict(dict)
    finder_ids = {finder_id for finder_id, _metric in counts}
    for finder_id in finder_ids:
        formal_low = _rate(counts[(finder_id, "formal_low_prior")])
        qonly_low = _rate(counts[(finder_id, "question_only_low_prior")])
        formal_impl = _rate(counts[(finder_id, "formal_implementation_signal")])
        qonly_impl = _rate(counts[(finder_id, "question_only_implementation_signal")])
        out[finder_id] = {
            "low_prior_formal_accuracy": formal_low,
            "low_prior_question_only_accuracy": qonly_low,
            "low_prior_lift": formal_low - qonly_low,
            "implementation_signal_formal_accuracy": formal_impl,
            "implementation_signal_question_only_accuracy": qonly_impl,
            "implementation_signal_lift": formal_impl - qonly_impl,
        }
    return out


def _load_affinity(path: Path) -> dict[str, dict[str, float]]:
    out = {}
    for row in _read_csv(path):
        if row["role"] != "finder":
            continue
        out[row["agent_id"]] = {
            "same_family_minus_different_family": _float(
                row["same_family_minus_different_family"]
            ),
            "same_base_minus_different_base": _float(row["same_base_minus_different_base"]),
        }
    return out


def _finder_rank_stability(path: Path) -> defaultdict[str, dict[str, float]]:
    maps: dict[tuple[str, str, str], dict[str, float]] = defaultdict(dict)
    for row in _read_csv(path):
        if row["gate"] != "all_runs":
            continue
        if row["metric"] != "total_bytes":
            continue
        if row["panel_id"] not in {"high_effort", "low_effort"}:
            continue
        key = (row["panel_id"], row["task_id"], row["finder_id"])
        maps[key][row["builder_id"]] = _float(row["cost"])

    by_panel_task: dict[tuple[str, str], list[str]] = defaultdict(list)
    for panel_id, task_id, finder_id in maps:
        by_panel_task[(panel_id, task_id)].append(finder_id)

    per_finder: dict[str, list[float]] = defaultdict(list)
    for (panel_id, task_id), finder_ids in by_panel_task.items():
        for left_index, left in enumerate(sorted(finder_ids)):
            for right in sorted(finder_ids)[left_index + 1 :]:
                value = _kendall_tau(
                    maps[(panel_id, task_id, left)],
                    maps[(panel_id, task_id, right)],
                )
                if not math.isnan(value):
                    per_finder[left].append(value)
                    per_finder[right].append(value)

    out: defaultdict[str, dict[str, float]] = defaultdict(dict)
    for finder_id, values in per_finder.items():
        ordered = sorted(values)
        out[finder_id] = {
            "rank_stability_mean_tau": sum(ordered) / len(ordered),
            "rank_stability_median_tau": _median(ordered),
        }
    return out


def _kendall_tau(left: dict[str, float], right: dict[str, float]) -> float:
    keys = sorted(set(left) & set(right))
    concordant = 0
    discordant = 0
    for i, first in enumerate(keys):
        for second in keys[i + 1 :]:
            left_sign = _sign(left[first] - left[second])
            right_sign = _sign(right[first] - right[second])
            if left_sign == 0 or right_sign == 0:
                continue
            if left_sign == right_sign:
                concordant += 1
            else:
                discordant += 1
    total = concordant + discordant
    if total == 0:
        return math.nan
    return (concordant - discordant) / total


def _core_gate_failures(row: dict[str, Any]) -> list[str]:
    checks = [
        (
            row["formal_run_ci95_low"] >= CORE_FORMAL_CI_LOW_MIN,
            f"formal CI low < {CORE_FORMAL_CI_LOW_MIN:.3f}",
        ),
        (
            row["formal_ci_width"] <= CORE_FORMAL_CI_WIDTH_MAX,
            f"formal CI width > {CORE_FORMAL_CI_WIDTH_MAX:.3f}",
        ),
        (
            row["mean_find_tokens"] <= CORE_MEAN_TOKENS_MAX,
            f"mean tokens > {CORE_MEAN_TOKENS_MAX}",
        ),
        (row["formal_minus_question_only"] > 0, "formal <= question-only"),
        (row["low_prior_lift"] > 0, "low-prior formal <= question-only"),
        (
            row["implementation_signal_lift"] > 0,
            "implementation-signal formal <= question-only",
        ),
        (
            row["rank_stability_mean_tau"] >= CORE_RANK_STABILITY_MEAN_TAU_MIN,
            f"rank-stability mean tau < {CORE_RANK_STABILITY_MEAN_TAU_MIN:.2f}",
        ),
        (
            abs(row["same_family_minus_different_family"]) <= AFFINITY_ABS_MAX,
            f"absolute same-family affinity > {AFFINITY_ABS_MAX:.2f}",
        ),
        (
            abs(row["same_base_minus_different_base"]) <= AFFINITY_ABS_MAX,
            f"absolute same-base affinity > {AFFINITY_ABS_MAX:.2f}",
        ),
    ]
    return [reason for passed, reason in checks if not passed]


def _audit_gate_failures(row: dict[str, Any]) -> list[str]:
    checks = [
        (
            row["formal_accuracy"] >= AUDIT_FORMAL_ACCURACY_MIN,
            f"formal accuracy < {AUDIT_FORMAL_ACCURACY_MIN:.3f}",
        ),
        (row["low_prior_lift"] >= AUDIT_LIFT_MIN, f"low-prior lift < {AUDIT_LIFT_MIN:.2f}"),
        (
            row["implementation_signal_lift"] >= AUDIT_LIFT_MIN,
            f"implementation-signal lift < {AUDIT_LIFT_MIN:.2f}",
        ),
        (
            row["mean_find_tokens"] <= AUDIT_MEAN_TOKENS_MAX,
            f"mean tokens > {AUDIT_MEAN_TOKENS_MAX}",
        ),
        (
            row["rank_stability_mean_tau"] >= AUDIT_RANK_STABILITY_MEAN_TAU_MIN,
            f"rank-stability mean tau < {AUDIT_RANK_STABILITY_MEAN_TAU_MIN:.2f}",
        ),
        (
            abs(row["same_family_minus_different_family"]) <= AFFINITY_ABS_MAX,
            f"absolute same-family affinity > {AFFINITY_ABS_MAX:.2f}",
        ),
        (
            abs(row["same_base_minus_different_base"]) <= AFFINITY_ABS_MAX,
            f"absolute same-base affinity > {AFFINITY_ABS_MAX:.2f}",
        ),
    ]
    return [reason for passed, reason in checks if not passed]


def _choose_audit_finders(rows: list[dict[str, Any]]) -> list[str]:
    candidates = [row for row in rows if row["audit_candidate"]]
    if not candidates:
        return []
    best = max(
        candidates,
        key=lambda row: (
            min(row["low_prior_lift"], row["implementation_signal_lift"]),
            row["formal_accuracy"],
            -row["mean_find_tokens"],
        ),
    )
    return [best["finder_id"]]


def _sort_selection_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    selection_rank = {
        "core_default": 0,
        "prior_stress_audit": 1,
        "not_selected": 2,
    }
    return sorted(
        rows,
        key=lambda row: (
            selection_rank[row["selection"]],
            -row["formal_accuracy"],
            row["mean_find_tokens"],
            row["finder_id"],
        ),
    )


def _write_markdown(path: Path, *, manifest: dict[str, Any], rows: list[dict[str, Any]]) -> None:
    selected_rows = [row for row in rows if row["selection"] != "not_selected"]
    excluded_rows = [row for row in rows if row["selection"] == "not_selected"]
    lines = [
        "# Core Finder Selection",
        "",
        f"- Selection id: `{manifest['selection_id']}`",
        f"- Script: `{manifest['analysis_script']}`",
        f"- Default core finders: {', '.join(f'`{x}`' for x in manifest['default_core_finders'])}",
        f"- Prior-stress audit finders: {', '.join(f'`{x}`' for x in manifest['prior_stress_audit_finders'])}",
        "",
        "## Interpretation",
        "",
        "The default core panel is intentionally smaller than the four highest formal-accuracy finders. The Opus and Codex finders have high artifact-conditioned accuracy, but their finder-specific question-only controls are saturated, so they are not clean routine instruments for future artifact-conditioned scoring in this task pack.",
        "",
        "Spec-only controls are also saturated. The selector records that fact as a validity limitation and uses finder-specific question-only lift, low-prior lift, and implementation-signal lift as the prior controls that can still discriminate finder behavior.",
        "",
        "## Selected Finders",
        "",
        "| role | finder | formal | q-only lift | low-prior lift | impl.-signal lift | mean tokens | tau | affinity |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for row in selected_rows:
        lines.append(
            "| "
            + " | ".join(
                [
                    row["selection"],
                    f"`{row['finder_id']}`",
                    _pct(row["formal_accuracy"]),
                    _pct(row["formal_minus_question_only"]),
                    _pct(row["low_prior_lift"]),
                    _pct(row["implementation_signal_lift"]),
                    f"{row['mean_find_tokens']:.0f}",
                    f"{row['rank_stability_mean_tau']:.3f}",
                    _pct(row["same_family_minus_different_family"]),
                ]
            )
            + " |"
        )
    lines.extend(
        [
            "",
            "## Non-Selected Summary",
            "",
            "| finder | formal | q-only lift | mean tokens | tau | main reason |",
            "| --- | ---: | ---: | ---: | ---: | --- |",
        ]
    )
    for row in sorted(excluded_rows, key=lambda r: r["formal_accuracy"], reverse=True):
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{row['finder_id']}`",
                    _pct(row["formal_accuracy"]),
                    _pct(row["formal_minus_question_only"]),
                    f"{row['mean_find_tokens']:.0f}",
                    f"{row['rank_stability_mean_tau']:.3f}",
                    row["selection_reason"],
                ]
            )
            + " |"
        )
    lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")


def _write_csv(path: Path, fieldnames: list[str], rows: list[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle, fieldnames=fieldnames, extrasaction="ignore", lineterminator="\n"
        )
        writer.writeheader()
        for row in rows:
            writer.writerow({key: _csv_value(row[key]) for key in fieldnames})


def _csv_value(value: Any) -> Any:
    if isinstance(value, float):
        return f"{value:.6f}"
    return value


def _read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle))


def _add(counter: list[int], correct: int) -> None:
    counter[0] += correct
    counter[1] += 1


def _rate(counter: list[int]) -> float:
    correct, total = counter
    if total == 0:
        return math.nan
    return correct / total


def _float(value: str) -> float:
    return float(value) if value else math.nan


def _median(values: list[float]) -> float:
    midpoint = len(values) // 2
    if len(values) % 2:
        return values[midpoint]
    return (values[midpoint - 1] + values[midpoint]) / 2


def _sign(value: float) -> int:
    return (value > 0) - (value < 0)


def _pct(value: float) -> str:
    return f"{value * 100:.1f}%"


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
