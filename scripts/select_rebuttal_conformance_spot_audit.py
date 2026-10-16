"""Reproduce the stratified consensus-prelabel spot-audit sample.

The script is read-only by default: it emits the selected CSV to stdout.  Use
``--check`` to compare the deterministic selection with the frozen rebuttal
manifest without rewriting either input.
"""

from __future__ import annotations

import argparse
import csv
import io
import random
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_INPUT = (
    ROOT
    / "runs_release"
    / "reports"
    / "artifact_conformance_consensus"
    / "final"
    / "artifact_conformance_final.csv"
)
DEFAULT_MANIFEST = (
    ROOT / "rebuttal" / "conformance_spot_audit" / "sample_manifest.csv"
)
DEFAULT_SEED = 20260725
TASKS = ("scratch_minidb", "scratch_nanoweb")
FAMILIES = ("claude", "codex", "mimo")
PER_STRATUM = 4
KNOWN_PILOT_FALSE_POSITIVES = {
    (
        "scratch_minidb",
        "build-scratch_minidb-claude_code_mimo_v2_5_low-"
        "t0-s20260501-51f333b3",
        "q_db_05",
    ),
    (
        "scratch_nanoweb",
        "build-scratch_nanoweb-claude_code_mimo_v2_5_high-"
        "t1-s20260502-44ad1601",
        "q_nw_07",
    ),
}
OUTPUT_FIELDS = (
    "sample_index",
    "stratum",
    "task_id",
    "artifact_id",
    "builder_id",
    "builder_family",
    "question_id",
    "question_type",
    "consensus_canonical_option",
    "gold_canonical_option",
    "source_requirement_id",
    "source_intention_id",
)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, default=DEFAULT_INPUT)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--seed", type=int, default=DEFAULT_SEED)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    selected = select_rows(read_csv(args.input), seed=args.seed)
    rendered = render_csv(selected)

    if args.check:
        expected = args.manifest.read_text(encoding="utf-8")
        if rendered != expected:
            raise SystemExit(
                f"selection differs from frozen manifest: {args.manifest}"
            )
        print(
            f"ok: {len(selected)} rows match {args.manifest.relative_to(ROOT)} "
            f"(seed={args.seed})"
        )
        return

    print(rendered, end="")


def read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as handle:
        return list(csv.DictReader(handle))


def select_rows(
    rows: list[dict[str, str]], *, seed: int
) -> list[dict[str, str | int]]:
    candidates = [
        row
        for row in rows
        if row["compile_pass"].lower() == "true"
        and row["label_source"] == "consensus_prelabel"
        and row["final_label"] == "implemented_gold"
        and (
            row["task_id"],
            row["artifact_id"],
            row["question_id"],
        )
        not in KNOWN_PILOT_FALSE_POSITIVES
    ]
    random.Random(seed).shuffle(candidates)

    selected: list[dict[str, str | int]] = []
    for task_id in TASKS:
        for builder_family in FAMILIES:
            pool = [
                row
                for row in candidates
                if row["task_id"] == task_id
                and row["builder_family"] == builder_family
            ]
            stratum_rows: list[dict[str, str]] = []
            seen_artifacts: set[str] = set()
            seen_questions: set[str] = set()
            for row in pool:
                if row["artifact_id"] in seen_artifacts:
                    continue
                if row["question_id"] in seen_questions:
                    continue
                stratum_rows.append(row)
                seen_artifacts.add(row["artifact_id"])
                seen_questions.add(row["question_id"])
                if len(stratum_rows) == PER_STRATUM:
                    break
            if len(stratum_rows) != PER_STRATUM:
                raise RuntimeError(
                    f"could select only {len(stratum_rows)} rows for "
                    f"{task_id}|{builder_family}"
                )
            for row in stratum_rows:
                selected.append(
                    {
                        "sample_index": len(selected) + 1,
                        "stratum": f"{task_id}|{builder_family}",
                        **{field: row[field] for field in OUTPUT_FIELDS[2:]},
                    }
                )

    if len(selected) != len(TASKS) * len(FAMILIES) * PER_STRATUM:
        raise AssertionError("unexpected sample size")
    return selected


def render_csv(rows: list[dict[str, str | int]]) -> str:
    buffer = io.StringIO(newline="")
    writer = csv.DictWriter(buffer, fieldnames=OUTPUT_FIELDS, lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
    return buffer.getvalue()


if __name__ == "__main__":
    main()
