#!/usr/bin/env python3
"""Sample correct finder answers for post-hoc artifact-evidence auditing."""

from __future__ import annotations

import argparse
import csv
import json
import random
from collections import defaultdict
from pathlib import Path
from typing import Any

from bnf.analysis.tables import PAPER_FINAL_PANEL_ID, agent_family, require_analysis_tables
from bnf.config import paths


AUDIT_SEED = 20260601
DEFAULT_SAMPLE_SIZE = 72
LOW_PRIOR_MANIFEST = Path("eandd_final/low_prior/manifest.json")
OUTPUT_NAME = "evidence_citation_audit_sample.csv"

OUTPUT_COLUMNS = (
    "sample_index",
    "stratum_id",
    "prior_status",
    "find_run_id",
    "build_run_id",
    "artifact_path",
    "task_id",
    "builder_agent",
    "builder_family",
    "finder_agent",
    "finder_family",
    "question_id",
    "selected_option",
    "presented_correct_key",
    "selected_text",
    "correct_text",
    "evidence_files",
    "evidence_symbols",
    "rationale",
    "audit_label",
    "supporting_note",
)


def main() -> None:
    args = _parse_args()
    p = paths()
    tables_dir = args.tables_dir or p.tables_dir
    reports_dir = args.reports_dir or p.reports_dir
    out_path = args.output or reports_dir / OUTPUT_NAME

    require_analysis_tables(tables_dir)
    low_prior_questions = _load_low_prior_questions(reports_dir)
    build_rows = _read_csv(tables_dir / "build_runs.csv")
    artifact_by_build = {row["build_run_id"]: row["artifact_path"] for row in build_rows}

    candidates = _candidate_rows(
        _read_csv(tables_dir / "question_answers.csv"),
        artifact_by_build=artifact_by_build,
        low_prior_questions=low_prior_questions,
    )
    sampled = _sample_rows(candidates, sample_size=args.sample_size, seed=args.seed)

    out_path.parent.mkdir(parents=True, exist_ok=True)
    _write_csv(out_path, sampled)
    print(f"wrote {out_path} ({len(sampled)} rows)")


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tables-dir", type=Path, help="Canonical analysis tables directory.")
    parser.add_argument("--reports-dir", type=Path, help="Release reports directory.")
    parser.add_argument("--output", type=Path, help="Audit sample CSV path.")
    parser.add_argument("--sample-size", type=int, default=DEFAULT_SAMPLE_SIZE)
    parser.add_argument("--seed", type=int, default=AUDIT_SEED)
    return parser.parse_args()


def _load_low_prior_questions(reports_dir: Path) -> set[tuple[str, str]]:
    path = reports_dir / LOW_PRIOR_MANIFEST
    manifest = json.loads(path.read_text(encoding="utf-8"))
    questions = manifest["question_subset"]["questions"]
    return {tuple(item.split(":", 1)) for item in questions}


def _candidate_rows(
    answer_rows: list[dict[str, str]],
    *,
    artifact_by_build: dict[str, str],
    low_prior_questions: set[tuple[str, str]],
) -> list[dict[str, str]]:
    rows: list[dict[str, str]] = []
    for row in answer_rows:
        if PAPER_FINAL_PANEL_ID not in row["panel_ids"].split("|"):
            continue
        if row["is_correct"].lower() != "true":
            continue
        key = (row["task_id"], row["question_id"])
        prior_status = "low_prior" if key in low_prior_questions else "high_prior"
        builder_family = agent_family(row["builder_id"])
        finder_family = agent_family(row["finder_id"])
        stratum_id = "|".join((row["task_id"], builder_family, finder_family, prior_status))
        rows.append(
            {
                "sample_index": "",
                "stratum_id": stratum_id,
                "prior_status": prior_status,
                "find_run_id": row["find_run_id"],
                "build_run_id": row["build_run_id"],
                "artifact_path": artifact_by_build[row["build_run_id"]],
                "task_id": row["task_id"],
                "builder_agent": row["builder_id"],
                "builder_family": builder_family,
                "finder_agent": row["finder_id"],
                "finder_family": finder_family,
                "question_id": row["question_id"],
                "selected_option": row["selected_option"],
                "presented_correct_key": row["correct_option"],
                "selected_text": row["selected_text"],
                "correct_text": row["correct_text"],
                "evidence_files": row["evidence_files"],
                "evidence_symbols": row["evidence_symbols"],
                "rationale": row["rationale"],
                "audit_label": "",
                "supporting_note": "",
            }
        )
    return rows


def _sample_rows(rows: list[dict[str, str]], *, sample_size: int, seed: int) -> list[dict[str, str]]:
    rng = random.Random(seed)
    by_stratum: dict[str, list[dict[str, str]]] = defaultdict(list)
    for row in rows:
        by_stratum[row["stratum_id"]].append(row)

    selected: list[dict[str, str]] = []
    selected_ids: set[tuple[str, str]] = set()
    per_stratum_target = max(1, sample_size // max(1, len(by_stratum)))
    for stratum_id in sorted(by_stratum):
        candidates = list(by_stratum[stratum_id])
        rng.shuffle(candidates)
        for row in candidates[:per_stratum_target]:
            selected.append(row)
            selected_ids.add((row["find_run_id"], row["question_id"]))

    if len(selected) < sample_size:
        remaining = [
            row
            for row in rows
            if (row["find_run_id"], row["question_id"]) not in selected_ids
        ]
        rng.shuffle(remaining)
        selected.extend(remaining[: sample_size - len(selected)])

    if len(selected) > sample_size:
        rng.shuffle(selected)
        selected = selected[:sample_size]

    selected.sort(key=lambda row: (row["stratum_id"], row["find_run_id"], row["question_id"]))
    for index, row in enumerate(selected, start=1):
        row["sample_index"] = str(index)
    return selected


def _read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle))


def _write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=list(OUTPUT_COLUMNS),
            extrasaction="raise",
            lineterminator="\n",
        )
        writer.writeheader()
        writer.writerows(rows)


if __name__ == "__main__":
    main()
