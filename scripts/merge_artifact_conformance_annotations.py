"""Merge consensus conformance prelabels with manual review annotations."""

# ruff: noqa: E402

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import sys
from collections import Counter, defaultdict
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src"
if str(SRC) not in sys.path:
    sys.path.insert(0, str(SRC))

from bnf.config import paths


FINAL_LABELS = {
    "implemented_gold",
    "implemented_non_gold",
    "absent_or_ambiguous",
    "documented_only",
    "not_applicable_or_invalid",
}
CONFIDENCE_VALUES = {"high", "medium", "low"}
KEY_FIELDS = ("task_id", "artifact_id", "question_id")
FINAL_FIELD_ORDER = [
    "task_id",
    "artifact_id",
    "artifact_path",
    "builder_id",
    "builder_family",
    "builder_effort",
    "build_trial_id",
    "compile_pass",
    "question_id",
    "question_type",
    "question_prompt",
    "gold_claim_polarity",
    "gold_canonical_option",
    "gold_text",
    "option_a",
    "option_b",
    "option_c",
    "option_d",
    "final_label",
    "label_source",
    "confidence",
    "evidence_files",
    "evidence_symbols",
    "rationale",
    "reviewer",
    "reviewed_at",
    "notes",
    "consensus_status",
    "preliminary_label",
    "consensus_canonical_option",
    "consensus_text",
    "consensus_reason",
    "model_vote_summary",
]
SUMMARY_FIELD_ORDER = [
    "task_id",
    "compile_pass",
    "builder_id",
    "artifact_question_pairs",
    "implemented_gold",
    "implemented_gold_rate",
    "implemented_non_gold",
    "implemented_non_gold_rate",
    "absent_or_ambiguous",
    "absent_or_ambiguous_rate",
    "documented_only",
    "documented_only_rate",
    "not_applicable_or_invalid",
    "not_applicable_or_invalid_rate",
]


def main() -> None:
    args = _parse_args()
    p = paths()
    consensus_dir = args.consensus_dir or (p.reports_dir / "artifact_conformance_consensus")
    annotations_path = args.annotations or (consensus_dir / "manual_review_annotations.csv")
    output_dir = args.output_dir or (consensus_dir / "final")
    output_dir.mkdir(parents=True, exist_ok=True)

    consensus_path = consensus_dir / "artifact_question_consensus.csv"
    manual_queue_path = consensus_dir / "manual_review_queue.csv"
    consensus_rows = _read_csv(consensus_path)
    manual_queue_rows = _read_csv(manual_queue_path)
    annotation_rows = _read_csv(annotations_path) if annotations_path.exists() else []

    annotations = _validated_annotations(annotation_rows)
    final_rows, missing_manual = _merged_rows(
        consensus_rows=consensus_rows,
        manual_queue_rows=manual_queue_rows,
        annotations=annotations,
    )
    missing_path = output_dir / "missing_manual_annotations.csv"
    if missing_manual:
        _write_missing(missing_path, missing_manual)
        print(
            f"missing {len(missing_manual)} manual annotations; see "
            f"{missing_path}",
            file=sys.stderr,
        )
        raise SystemExit(1)
    if missing_path.exists():
        missing_path.unlink()

    summary_rows = _summary_rows(final_rows)
    _write_csv(output_dir / "artifact_conformance_final.csv", final_rows, FINAL_FIELD_ORDER)
    _write_csv(output_dir / "summary.csv", summary_rows, SUMMARY_FIELD_ORDER)
    manifest = {
        "analysis_id": "artifact_conformance_final_v1",
        "analysis_script": "scripts/merge_artifact_conformance_annotations.py",
        "analysis_command": " ".join(sys.argv),
        "generated_at": datetime.now(UTC).isoformat(),
        "input_paths": [
            _display_path(consensus_path),
            _display_path(manual_queue_path),
            _display_path(annotations_path),
        ],
        "input_hashes": {
            _display_path(path): _sha256(path)
            for path in (consensus_path, manual_queue_path, annotations_path)
            if path.exists()
        },
        "output_dir": _display_path(output_dir),
        "row_counts": {
            "consensus_rows": len(consensus_rows),
            "manual_queue_rows": len(manual_queue_rows),
            "manual_annotation_rows": len(annotation_rows),
            "final_rows": len(final_rows),
        },
        "label_sources": dict(Counter(row["label_source"] for row in final_rows)),
        "final_labels": dict(Counter(row["final_label"] for row in final_rows)),
    }
    (output_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    _write_markdown(output_dir / "README.md", manifest=manifest, summary_rows=summary_rows)
    print(f"wrote {output_dir}")


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Merge artifact conformance consensus prelabels with manual annotations."
    )
    parser.add_argument(
        "--consensus-dir",
        type=Path,
        help="Directory containing artifact_question_consensus.csv and manual_review_queue.csv.",
    )
    parser.add_argument(
        "--annotations",
        type=Path,
        help="Manual annotation CSV. Defaults to consensus-dir/manual_review_annotations.csv.",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        help="Output directory. Defaults to consensus-dir/final.",
    )
    return parser.parse_args()


def _read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as handle:
        return list(csv.DictReader(handle))


def _write_csv(path: Path, rows: list[dict[str, Any]], preferred: list[str]) -> None:
    keys = {key for row in rows for key in row}
    fieldnames = [field for field in preferred if field in keys]
    fieldnames.extend(sorted(keys - set(fieldnames)))
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(rows)


def _validated_annotations(rows: list[dict[str, str]]) -> dict[tuple[str, str, str], dict[str, str]]:
    annotations = {}
    errors = []
    for index, row in enumerate(rows, start=2):
        label = row.get("final_label", "").strip()
        if not label:
            continue
        key = _key(row)
        if key in annotations:
            errors.append(f"duplicate annotation at line {index}: {key}")
        if label not in FINAL_LABELS:
            errors.append(f"invalid final_label at line {index}: {label}")
        confidence = row.get("confidence", "").strip()
        if confidence not in CONFIDENCE_VALUES:
            errors.append(f"invalid confidence at line {index}: {confidence}")
        if not row.get("rationale", "").strip():
            errors.append(f"missing rationale at line {index}: {key}")
        annotations[key] = {**row, "final_label": label, "confidence": confidence}
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        raise SystemExit(1)
    return annotations


def _merged_rows(
    *,
    consensus_rows: list[dict[str, str]],
    manual_queue_rows: list[dict[str, str]],
    annotations: dict[tuple[str, str, str], dict[str, str]],
) -> tuple[list[dict[str, str]], list[dict[str, str]]]:
    manual_keys = {_key(row) for row in manual_queue_rows}
    final_rows = []
    missing_manual = []
    for row in consensus_rows:
        key = _key(row)
        annotation = annotations.get(key)
        if key in manual_keys:
            if not annotation:
                missing_manual.append(row)
                continue
            final_rows.append(_manual_final_row(row, annotation, label_source="manual_review"))
        elif annotation:
            final_rows.append(_manual_final_row(row, annotation, label_source="manual_override"))
        else:
            final_rows.append(_consensus_final_row(row))
    return final_rows, missing_manual


def _consensus_final_row(row: dict[str, str]) -> dict[str, str]:
    return {
        **row,
        "final_label": row["preliminary_label"],
        "label_source": "consensus_prelabel",
        "confidence": "",
        "evidence_files": "",
        "evidence_symbols": "",
        "rationale": "Stable selected-finder consensus prelabel; not an independent audit.",
        "reviewer": "",
        "reviewed_at": "",
        "notes": "",
    }


def _manual_final_row(
    row: dict[str, str],
    annotation: dict[str, str],
    *,
    label_source: str,
) -> dict[str, str]:
    return {
        **row,
        "final_label": annotation["final_label"],
        "label_source": label_source,
        "confidence": annotation["confidence"],
        "evidence_files": annotation.get("evidence_files", ""),
        "evidence_symbols": annotation.get("evidence_symbols", ""),
        "rationale": annotation.get("rationale", ""),
        "reviewer": annotation.get("reviewer", ""),
        "reviewed_at": annotation.get("reviewed_at", ""),
        "notes": annotation.get("notes", ""),
    }


def _summary_rows(rows: list[dict[str, str]]) -> list[dict[str, Any]]:
    groups: dict[tuple[str, str, str], list[dict[str, str]]] = defaultdict(list)
    for row in rows:
        groups[("all", "all", "all")].append(row)
        groups[(row["task_id"], "all", "all")].append(row)
        groups[(row["task_id"], row["compile_pass"], "all")].append(row)
        groups[(row["task_id"], row["compile_pass"], row["builder_id"])].append(row)

    out = []
    for (task_id, compile_pass, builder_id), group_rows in sorted(groups.items()):
        counts = Counter(row["final_label"] for row in group_rows)
        out_row = {
            "task_id": task_id,
            "compile_pass": compile_pass,
            "builder_id": builder_id,
            "artifact_question_pairs": len(group_rows),
        }
        for label in sorted(FINAL_LABELS):
            out_row[label] = counts[label]
            out_row[f"{label}_rate"] = _fmt_float(counts[label] / len(group_rows))
        out.append(out_row)
    return out


def _write_missing(path: Path, rows: list[dict[str, str]]) -> None:
    _write_csv(
        path,
        [
            {
                "task_id": row["task_id"],
                "artifact_id": row["artifact_id"],
                "question_id": row["question_id"],
                "gold_canonical_option": row["gold_canonical_option"],
                "gold_text": row["gold_text"],
                "consensus_reason": row["consensus_reason"],
                "model_vote_summary": row["model_vote_summary"],
            }
            for row in rows
        ],
        [
            "task_id",
            "artifact_id",
            "question_id",
            "gold_canonical_option",
            "gold_text",
            "consensus_reason",
            "model_vote_summary",
        ],
    )


def _write_markdown(
    path: Path,
    *,
    manifest: dict[str, Any],
    summary_rows: list[dict[str, Any]],
) -> None:
    overall = next(row for row in summary_rows if row["task_id"] == "all")
    lines = [
        "# Final Artifact Conformance",
        "",
        "This table merges stable consensus prelabels with manual review annotations.",
        "",
        "## Overall Counts",
        "",
        "| label | count | rate |",
        "| --- | ---: | ---: |",
    ]
    for label in sorted(FINAL_LABELS):
        lines.append(f"| `{label}` | {overall[label]} | {overall[f'{label}_rate']} |")
    lines.extend(
        [
            "",
            "## Source Counts",
            "",
            "| source | rows |",
            "| --- | ---: |",
        ]
    )
    for source, count in sorted(manifest["label_sources"].items()):
        lines.append(f"| `{source}` | {count} |")
    lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")


def _key(row: dict[str, str]) -> tuple[str, str, str]:
    return tuple(row[field] for field in KEY_FIELDS)


def _fmt_float(value: float) -> str:
    return f"{value:.6f}"


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _display_path(path: Path) -> str:
    try:
        return str(path.resolve().relative_to(ROOT))
    except ValueError:
        return str(path)


if __name__ == "__main__":
    main()
