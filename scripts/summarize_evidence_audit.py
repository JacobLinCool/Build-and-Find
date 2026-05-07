#!/usr/bin/env python3
"""Summarize the post-hoc artifact-evidence audit sample."""

from __future__ import annotations

import argparse
import csv
from collections import Counter, defaultdict
from pathlib import Path

from bnf.config import paths


INPUT_NAME = "evidence_citation_audit_sample.csv"
OUTPUT_NAME = "evidence_citation_audit.md"
ALLOWED_LABELS = (
    "Supported",
    "Partially supported",
    "Unsupported",
    "Prior-like",
)


def main() -> None:
    args = _parse_args()
    p = paths()
    reports_dir = args.reports_dir or p.reports_dir
    input_path = args.input or reports_dir / INPUT_NAME
    output_path = args.output or reports_dir / OUTPUT_NAME

    rows = _read_csv(input_path)
    _validate_rows(rows)
    output_path.write_text(_markdown(rows), encoding="utf-8")
    print(f"wrote {output_path}")


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reports-dir", type=Path, help="Release reports directory.")
    parser.add_argument("--input", type=Path, help="Completed audit sample CSV.")
    parser.add_argument("--output", type=Path, help="Markdown summary path.")
    return parser.parse_args()


def _read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle))


def _validate_rows(rows: list[dict[str, str]]) -> None:
    if not rows:
        raise ValueError("audit sample is empty")
    invalid = [
        row["sample_index"]
        for row in rows
        if row.get("audit_label", "") not in ALLOWED_LABELS
    ]
    if invalid:
        sample = ", ".join(invalid[:8])
        raise ValueError(f"rows with missing or invalid audit_label: {sample}")


def _markdown(rows: list[dict[str, str]]) -> str:
    lines = [
        "# Post-hoc Artifact-Evidence Audit",
        "",
        f"Sampled correct answers: {len(rows)}",
        "",
        "Labels: Supported, Partially supported, Unsupported, Prior-like.",
        "",
        "## Overall",
        "",
        *_table(_counts(rows), "Slice"),
        "",
        "## Slices",
        "",
    ]
    for title, key in (
        ("Prior status", "prior_status"),
        ("Task", "task_id"),
        ("Builder family", "builder_family"),
        ("Finder family", "finder_family"),
    ):
        lines.extend([f"### {title}", "", *_slice_table(rows, key), ""])
    return "\n".join(lines).rstrip() + "\n"


def _slice_table(rows: list[dict[str, str]], key: str) -> list[str]:
    by_slice: dict[str, list[dict[str, str]]] = defaultdict(list)
    for row in rows:
        by_slice[row[key]].append(row)
    table_rows = []
    for value in sorted(by_slice):
        counts = _counts(by_slice[value])
        table_rows.append(
            {
                "slice": value,
                "N": len(by_slice[value]),
                **counts,
            }
        )
    return _render_table(table_rows, "Slice")


def _counts(rows: list[dict[str, str]]) -> dict[str, int]:
    counts = Counter(row["audit_label"] for row in rows)
    return {label: counts[label] for label in ALLOWED_LABELS}


def _table(counts: dict[str, int], label_header: str) -> list[str]:
    row = {"slice": "All sampled correct answers", "N": sum(counts.values()), **counts}
    return _render_table([row], label_header)


def _render_table(rows: list[dict[str, int | str]], label_header: str) -> list[str]:
    lines = [
        f"| {label_header} | N | Supported | Partial | Unsupported | Prior-like |",
        "| --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    for row in rows:
        lines.append(
            f"| {row['slice']} | {row['N']} | {row['Supported']} | "
            f"{row['Partially supported']} | {row['Unsupported']} | {row['Prior-like']} |"
        )
    return lines


if __name__ == "__main__":
    main()
