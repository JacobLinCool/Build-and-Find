#!/usr/bin/env python3
"""Clustered bootstrap intervals for calibration accuracy differences."""

from __future__ import annotations

import csv
import json
import random
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from bnf.analysis.tables import ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID, PAPER_FINAL_PANEL_ID
from bnf.config import paths


BOOTSTRAP_SEED = 20260602
BOOTSTRAP_SAMPLES = 2000
OUTPUT_NAME = "bootstrap_lift.csv"


def main() -> None:
    p = paths()
    answers = _read_csv(p.tables_dir / "question_answers.csv")
    out_dir = p.reports_dir / "eandd_final" / "validity_stress"
    out_dir.mkdir(parents=True, exist_ok=True)
    low_prior_questions = _load_low_prior_questions(p.reports_dir)

    rows = [
        _bootstrap_row(
            "artifact_present_minus_question_only",
            treatment_rows=[
                row for row in answers if _in_panel(row, ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID)
            ],
            control_rows=[row for row in answers if _in_panel(row, "question_only")],
        ),
        _bootstrap_row(
            "compile_pass_minus_question_only",
            treatment_rows=[row for row in answers if _in_panel(row, PAPER_FINAL_PANEL_ID)],
            control_rows=[row for row in answers if _in_panel(row, "question_only")],
        ),
        _bootstrap_row(
            "low_prior_compile_pass_minus_question_only",
            treatment_rows=[
                row
                for row in answers
                if _in_panel(row, PAPER_FINAL_PANEL_ID)
                and (row["task_id"], row["question_id"]) in low_prior_questions
            ],
            control_rows=[
                row
                for row in answers
                if _in_panel(row, "question_only")
                and (row["task_id"], row["question_id"]) in low_prior_questions
            ],
        ),
    ]

    _write_csv(out_dir / OUTPUT_NAME, rows)
    manifest_path = out_dir / "bootstrap_lift_manifest.json"
    manifest = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "analysis_script": "scripts/analyze_eandd_calibration_uncertainty.py",
        "input_table": _display_path(p.tables_dir / "question_answers.csv"),
        "output_table": _display_path(out_dir / OUTPUT_NAME),
        "bootstrap_seed": BOOTSTRAP_SEED,
        "bootstrap_samples": BOOTSTRAP_SAMPLES,
        "cluster_unit": "task_id + question_id",
        "interval": "percentile 95% clustered bootstrap over question clusters",
    }
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"wrote {out_dir / OUTPUT_NAME}")


def _bootstrap_row(
    contrast_id: str,
    *,
    treatment_rows: list[dict[str, str]],
    control_rows: list[dict[str, str]],
) -> dict[str, Any]:
    treatment_by_cluster = _counts_by_cluster(treatment_rows)
    control_by_cluster = _counts_by_cluster(control_rows)
    clusters = sorted(set(treatment_by_cluster) & set(control_by_cluster))
    if not clusters:
        raise RuntimeError(f"{contrast_id}: no shared clusters")

    observed = _difference(treatment_by_cluster, control_by_cluster, clusters)
    rng = random.Random(f"{BOOTSTRAP_SEED}:{contrast_id}")
    samples: list[float] = []
    for _ in range(BOOTSTRAP_SAMPLES):
        draw = [clusters[rng.randrange(len(clusters))] for _ in clusters]
        samples.append(_difference(treatment_by_cluster, control_by_cluster, draw))
    samples.sort()
    lo = samples[int(0.025 * (len(samples) - 1))]
    hi = samples[int(0.975 * (len(samples) - 1))]

    return {
        "contrast_id": contrast_id,
        "cluster_unit": "task_question",
        "clusters": len(clusters),
        "treatment_answer_rows": sum(treatment_by_cluster[cluster][1] for cluster in clusters),
        "control_answer_rows": sum(control_by_cluster[cluster][1] for cluster in clusters),
        "difference": f"{observed:.6f}",
        "ci_low": f"{lo:.6f}",
        "ci_high": f"{hi:.6f}",
        "bootstrap_samples": BOOTSTRAP_SAMPLES,
        "bootstrap_seed": BOOTSTRAP_SEED,
    }


def _counts_by_cluster(rows: list[dict[str, str]]) -> dict[tuple[str, str], tuple[int, int]]:
    grouped: dict[tuple[str, str], list[int]] = defaultdict(lambda: [0, 0])
    for row in rows:
        counts = grouped[(row["task_id"], row["question_id"])]
        counts[0] += int(_as_bool(row["is_correct"]))
        counts[1] += 1
    return {cluster: (counts[0], counts[1]) for cluster, counts in grouped.items()}


def _difference(
    treatment_by_cluster: dict[tuple[str, str], tuple[int, int]],
    control_by_cluster: dict[tuple[str, str], tuple[int, int]],
    clusters: list[tuple[str, str]],
) -> float:
    treatment_correct = 0
    treatment_total = 0
    control_correct = 0
    control_total = 0
    for cluster in clusters:
        treatment_correct_i, treatment_total_i = treatment_by_cluster[cluster]
        control_correct_i, control_total_i = control_by_cluster[cluster]
        treatment_correct += treatment_correct_i
        treatment_total += treatment_total_i
        control_correct += control_correct_i
        control_total += control_total_i
    return treatment_correct / treatment_total - control_correct / control_total


def _load_low_prior_questions(reports_dir: Path) -> set[tuple[str, str]]:
    manifest = json.loads(
        (reports_dir / "eandd_final" / "low_prior" / "manifest.json").read_text(encoding="utf-8")
    )
    return {
        tuple(item.split(":", 1))
        for item in manifest["question_subset"]["questions"]
    }


def _read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle))


def _write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rows[0]), lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


def _in_panel(row: dict[str, str], panel_id: str) -> bool:
    return panel_id in row.get("panel_ids", "").split("|")


def _as_bool(value: str) -> bool:
    return value.lower() in {"1", "true", "yes"}


def _display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(Path.cwd()).as_posix()
    except ValueError:
        return path.resolve().as_posix()


if __name__ == "__main__":
    main()
