#!/usr/bin/env python3
"""Option-free check: free-text answers with and without artifact access.

The option-free check asks eight of the ten frozen low-prior questions without
answer choices. Two finders answered each question as a free-text claim, once with
a released build artifact in the workspace (``artifact_open``) and once from the
question alone (``question_only_open``), over three trials. The author graded the
96 claims against a rubric fixed before the runs.

This script reads the release copy under ``reports/option_free_check/`` (session
records, grading key, the frozen pass-1 and pass-2 grading sheets, and the LLM
labels kept for a consistency check), the released tables, and the implementation
labels. It writes ``summary.json`` and ``report.md`` next to the inputs with every
option-free value printed in the paper. Before counting, it checks that the
grading sheets reproduce the recorded answers in the frozen shuffle order, that
the records ask the frozen low-prior questions without the negative-contrast
items, and that the deterministic artifact rule selects the two artifacts the
sessions used.

Two label sensitivities drop, from both conditions, the question pairs whose
implementation label changes: under the nine observed source-audit errors, and
under the adjudicated labels (v1.1 labels with the decisions on all full-audit
flags). Both overlays are built in memory with the helpers of
``analyze_rebuttal_label_sensitivity.py``; the released label file is never
modified.
"""

# ruff: noqa: E402

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import sys
from collections import Counter
from collections.abc import Callable, Iterable, Sequence
from dataclasses import dataclass, replace
from fractions import Fraction
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from bnf.analysis.tables import PAPER_FINAL_PANEL_ID
from bnf.config import Paths, paths
from scripts import analyze_rebuttal_label_sensitivity as sensitivity
from scripts.analyze_artifact_conformance_impact import (
    _apply_label_corrections,
    _artifact_question_key,
)
from scripts.analyze_control_exclusions import is_half_tie, percent, signed_points


ANALYSIS_ID = "option_free_check_v1"
OUTPUT_PARTS = ("option_free_check",)
RECORDS_DIR = "records"
ARTIFACT_OPEN = "artifact_open"
QUESTION_ONLY_OPEN = "question_only_open"
CONDITIONS = (ARTIFACT_OPEN, QUESTION_ONLY_OPEN)
FINDERS = ("codex_gpt5_5_high", "claude_code_opus_4_7_high")
TASK_IDS = ("scratch_minidb", "scratch_nanoweb")
TRIALS = ("0", "1", "2")
SHUFFLE_SEED = 20260725
NEGATIVE_CONTRAST = "negative_contrast"
QUESTION_ONLY_PANEL_ID = "question_only"
CONTROL_FAMILY = "control"
IMPLEMENTED_GOLD = "implemented_gold"
CORRECT = "correct"
CLAIM_LABELS = ("correct", "incorrect", "unanswered")
EVIDENCE_LABELS = ("sufficient", "insufficient", "no_evidence_cited", "not_applicable")
SUFFICIENT = "sufficient"
KEY_FIELDS = (
    "grading_id",
    "open_find_run_id",
    "condition",
    "finder_id",
    "trial_id",
    "seed",
    "source_build_run_id",
)
SHEET_CONTENT_FIELDS = (
    "task_id",
    "question_id",
    "question_prompt",
    "claim",
    "evidence_files",
    "evidence_symbols",
    "rationale",
)
SHEET_FIELDS = (
    "shuffle_index",
    "grading_id",
    *SHEET_CONTENT_FIELDS,
    "claim_correct",
    "evidence_sufficient",
    "grader_note",
)
LLM_FIELDS = ("grading_id", "claim_correct", "evidence_sufficient", "note")
OBSERVED_ERRORS = "observed_errors"
ADJUDICATED = "adjudicated"
LABEL_SCENARIOS = (OBSERVED_ERRORS, ADJUDICATED)
SCENARIO_DESCRIPTIONS = {
    OBSERVED_ERRORS: "the nine observed source-audit errors",
    ADJUDICATED: "the adjudicated labels (decisions on all full-audit flags)",
}
MONTH_NAMES = (
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
)

Pair = tuple[str, str]
LabelKey = tuple[str, str, str]


@dataclass(frozen=True)
class Record:
    run_id: str
    condition: str
    task_id: str
    finder_id: str
    trial_id: str
    seed: str
    source_build_run_id: str
    created_at: str
    question_ids: tuple[str, ...]
    answers: dict[str, dict[str, Any]]
    vendor_total_tokens: int
    wall_clock_seconds: float


@dataclass(frozen=True)
class Response:
    grading_id: str
    run_id: str
    condition: str
    finder_id: str
    trial_id: str
    task_id: str
    question_id: str
    claim_label: str
    pass2_claim_label: str
    evidence_label: str
    cites_evidence: bool
    llm_claim_label: str
    llm_evidence_label: str
    has_note: bool

    @property
    def pair(self) -> Pair:
        return (self.task_id, self.question_id)

    @property
    def correct(self) -> bool:
        return self.claim_label == CORRECT


@dataclass(frozen=True)
class Rate:
    correct: int
    total: int

    @classmethod
    def of(cls, flags: Iterable[bool]) -> Rate:
        values = [bool(flag) for flag in flags]
        return cls(sum(values), len(values))

    @property
    def value(self) -> Fraction:
        if not self.total:
            raise ZeroDivisionError("rate over zero rows")
        return Fraction(self.correct, self.total)

    @property
    def count(self) -> str:
        return f"{self.correct}/{self.total}"


@dataclass(frozen=True)
class Comparison:
    scope: str
    artifact_open: Rate
    question_only_open: Rate

    @property
    def difference(self) -> Fraction:
        return self.artifact_open.value - self.question_only_open.value


@dataclass(frozen=True)
class ArtifactRule:
    finder_families: tuple[str, ...]
    eligible: dict[str, list[str]]
    selected: dict[str, str]
    used: dict[str, str]

    @property
    def reproduced(self) -> bool:
        return self.selected == self.used


@dataclass(frozen=True)
class MultipleChoice:
    artifact_present: Rate
    question_only: Rate
    artifact_present_runs: tuple[str, ...]
    question_only_runs: tuple[str, ...]
    question_only_runs_excluded: int
    run_dates: tuple[str, ...]


@dataclass(frozen=True)
class LabelOverlay:
    released: dict[LabelKey, str]
    scenarios: dict[str, dict[LabelKey, str]]
    corrections: dict[str, int]
    flags: dict[LabelKey, str]
    decisions: dict[LabelKey, str]


@dataclass(frozen=True)
class PairLabel:
    task_id: str
    question_id: str
    artifact_id: str
    released: str
    flag: str
    decision: str
    scenario_labels: dict[str, str]

    @property
    def pair(self) -> Pair:
        return (self.task_id, self.question_id)

    def changed(self, scenario: str) -> bool:
        return self.scenario_labels[scenario] != self.released


@dataclass(frozen=True)
class Sensitivity:
    scenario: str
    changed_pairs: tuple[Pair, ...]
    comparison: Comparison


@dataclass(frozen=True)
class ArtifactLabels:
    task_id: str
    artifact_id: str
    pairs: int
    released_gold: int
    flagged: int
    adjudicated_gold: int


@dataclass(frozen=True)
class Grading:
    correctness_unchanged: Rate
    evidence_labels: dict[str, dict[str, int]]
    artifact_sufficient: Rate
    sufficient_among_correct: Rate
    cited_evidence_rows: dict[str, Rate]
    grader_notes: int
    llm_correctness_agreement: Rate
    llm_correct_author_incorrect: int
    llm_incorrect_author_correct: int
    llm_evidence_agreement: Rate


@dataclass(frozen=True)
class PrintedValue:
    value_id: str
    printed: str
    exact: str
    source: str
    half_tie: bool = False


@dataclass(frozen=True)
class Analysis:
    inputs: dict[str, Path]
    record_paths: list[Path]
    generated_at: str | None
    records: dict[str, Record]
    questions: dict[str, tuple[str, ...]]
    frozen_low_prior: tuple[Pair, ...]
    excluded_questions: tuple[Pair, ...]
    rule: ArtifactRule
    responses: list[Response]
    comparisons: dict[str, Comparison]
    multiple_choice: MultipleChoice
    still_low_prior: tuple[Pair, ...]
    pair_labels: list[PairLabel]
    sensitivities: list[Sensitivity]
    artifacts: list[ArtifactLabels]
    overlay_corrections: dict[str, int]
    grading: Grading
    printed: list[PrintedValue]

    @property
    def pairs(self) -> tuple[Pair, ...]:
        return tuple((task, question) for task in TASK_IDS for question in self.questions[task])


def main() -> None:
    args = _parse_args()
    p = paths()
    input_dir = (args.input_dir or p.reports_dir.joinpath(*OUTPUT_PARTS)).resolve()
    out_dir = (args.output_dir or input_dir).resolve()
    analysis = analyze(p, input_dir=input_dir)
    write_outputs(analysis, out_dir)
    print(f"wrote {_display_path(out_dir)}")


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--input-dir",
        type=Path,
        default=None,
        help="Option-free release copy (default: reports/option_free_check).",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=None,
        help="Output directory (default: the input directory).",
    )
    return parser.parse_args()


# --------------------------------------------------------------------------- inputs


def analysis_inputs(p: Paths, *, input_dir: Path) -> dict[str, Path]:
    labels_dir = p.reports_dir / "artifact_conformance_consensus" / "final"
    report_dir = p.reports_dir / "eandd_final"
    full_audit_dir = p.root / "rebuttal" / "full_conformance_audit"
    spot_audit_dir = p.root / "rebuttal" / "conformance_spot_audit"
    return {
        "grading_key": input_dir / "grading_key.csv",
        "grading_pass1": input_dir / "grading_sheet.pass1.csv",
        "grading_pass2": input_dir / "grading_sheet.pass2.csv",
        "llm_labels": input_dir / "llm_prelabels.csv",
        "question_answers": p.tables_dir / "question_answers.csv",
        "find_runs": p.tables_dir / "find_runs.csv",
        "build_runs": p.tables_dir / "build_runs.csv",
        "table_manifest": p.tables_dir / "MANIFEST.json",
        "final_labels": labels_dir / "artifact_conformance_final.csv",
        "label_corrections": labels_dir / "label_corrections_v1_1.csv",
        "low_prior_manifest": report_dir / "low_prior" / "manifest.json",
        "control_low_prior": report_dir / "control_exclusions" / "low_prior_questions.csv",
        "control_exclusions": (
            report_dir / "control_exclusions" / "excluded_question_only_runs.csv"
        ),
        "spot_audit_minidb": spot_audit_dir / "minidb_adjudication.csv",
        "spot_audit_nanoweb": spot_audit_dir / "nanoweb_adjudication.csv",
        "spot_audit_sample_manifest": spot_audit_dir / "sample_manifest.csv",
        "full_audit_disagreements": full_audit_dir / "disagreements.csv",
        "author_decisions": full_audit_dir / "author_decisions.csv",
        "model_assisted_decisions": full_audit_dir / "model_assisted_decisions.csv",
    }


def load_records(record_paths: Sequence[Path]) -> dict[str, Record]:
    """Read the sanitized session records; every record must be a finished session."""
    records: dict[str, Record] = {}
    for path in record_paths:
        raw = _read_json(path)
        run_id = raw.get("open_find_run_id", "")
        if path.stem != run_id:
            raise ValueError(f"{path.name}: file name does not match run id {run_id!r}")
        if raw.get("record_type") != "open_find_run" or raw.get("status") != "ok":
            raise ValueError(f"{run_id}: expected an open_find_run record with status ok")
        usage = raw.get("usage") or {}
        if "trace_path" in usage:
            raise ValueError(f"{run_id}: release records drop usage.trace_path")
        condition = raw["condition"]
        if condition not in CONDITIONS:
            raise ValueError(f"{run_id}: unknown condition {condition!r}")
        source = raw.get("source_build_run_id") or ""
        if (condition == ARTIFACT_OPEN) != bool(source):
            raise ValueError(f"{run_id}: only artifact_open records name a source artifact")
        answers = {answer["question_id"]: answer for answer in raw["answers"]}
        question_ids = tuple(raw["question_ids"])
        if tuple(answers) != question_ids:
            raise ValueError(f"{run_id}: answers do not follow question_ids")
        manifest = raw["manifest"]
        records[run_id] = Record(
            run_id=run_id,
            condition=condition,
            task_id=raw["task_id"],
            finder_id=manifest["agent"]["id"],
            trial_id=str(manifest["trial_id"]),
            seed=str(manifest["seed"]),
            source_build_run_id=source,
            created_at=manifest["created_at"],
            question_ids=question_ids,
            answers=answers,
            vendor_total_tokens=int((usage.get("vendor_usage") or {}).get("total_tokens") or 0),
            wall_clock_seconds=float(usage.get("wall_clock_seconds") or 0.0),
        )
    return records


def check_matrix(records: dict[str, Record]) -> dict[str, tuple[str, ...]]:
    """Require one record per task x condition x finder x trial; return questions per task."""
    cells = Counter(
        (record.task_id, record.condition, record.finder_id, record.trial_id)
        for record in records.values()
    )
    expected = {
        (task, condition, finder, trial)
        for task in TASK_IDS
        for condition in CONDITIONS
        for finder in FINDERS
        for trial in TRIALS
    }
    if set(cells) != expected or any(count != 1 for count in cells.values()):
        raise ValueError("records do not cover each task x condition x finder x trial once")
    questions: dict[str, tuple[str, ...]] = {}
    for task in TASK_IDS:
        asked = {record.question_ids for record in records.values() if record.task_id == task}
        if len(asked) != 1:
            raise ValueError(f"{task}: records ask different questions")
        questions[task] = asked.pop()
    return questions


def used_artifacts(records: dict[str, Record]) -> dict[str, str]:
    used: dict[str, str] = {}
    for task in TASK_IDS:
        sources = {
            record.source_build_run_id
            for record in records.values()
            if record.task_id == task and record.condition == ARTIFACT_OPEN
        }
        if len(sources) != 1:
            raise ValueError(f"{task}: artifact_open records use {len(sources)} artifacts")
        used[task] = sources.pop()
    return used


def check_question_selection(
    frozen: Iterable[Pair],
    question_types: dict[Pair, str],
    questions: dict[str, tuple[str, ...]],
) -> tuple[Pair, ...]:
    """The records ask the frozen low-prior set minus its negative-contrast items."""
    frozen_set = set(frozen)
    negative = {pair for pair in frozen_set if question_types[pair] == NEGATIVE_CONTRAST}
    asked = {(task, question) for task, ids in questions.items() for question in ids}
    if asked != frozen_set - negative:
        raise ValueError(
            "records do not ask the frozen low-prior questions without negative-contrast items"
        )
    return tuple(sorted(negative))


def artifact_rule(
    build_rows: Sequence[dict[str, str]],
    label_rows: Iterable[dict[str, str]],
    *,
    finder_families: Iterable[str],
    used: dict[str, str],
) -> ArtifactRule:
    """Replay the frozen rule: compile pass, full implemented-gold coverage under the
    released labels, a builder family distinct from both finder families, and the
    lexicographically first build run id."""
    families = tuple(sorted(set(finder_families)))
    gold: Counter[str] = Counter()
    total: Counter[str] = Counter()
    for row in label_rows:
        total[row["artifact_id"]] += 1
        gold[row["artifact_id"]] += row["final_label"] == IMPLEMENTED_GOLD
    eligible: dict[str, list[str]] = {}
    for task in TASK_IDS:
        eligible[task] = sorted(
            row["build_run_id"]
            for row in build_rows
            if row["task_id"] == task
            and _as_bool(row["compile_pass"])
            and row["builder_family"] not in {*families, CONTROL_FAMILY}
            and total[row["build_run_id"]]
            and gold[row["build_run_id"]] == total[row["build_run_id"]]
        )
    selected = {task: ids[0] for task, ids in eligible.items() if ids}
    return ArtifactRule(finder_families=families, eligible=eligible, selected=selected, used=used)


def shuffle_hash(run_id: str, question_id: str) -> str:
    return hashlib.sha256(f"{SHUFFLE_SEED}|{run_id}|{question_id}".encode()).hexdigest()


def sheet_content(record: Record, question_id: str) -> dict[str, str]:
    """The grading-sheet columns a record answer produces."""
    answer = record.answers[question_id]
    return {
        "task_id": record.task_id,
        "question_id": question_id,
        "question_prompt": answer["question_prompt"],
        "claim": answer.get("claim") or "",
        "evidence_files": "|".join(answer.get("evidence_files") or []),
        "evidence_symbols": "|".join(answer.get("evidence_symbols") or []),
        "rationale": answer.get("rationale") or "",
    }


def load_responses(inputs: dict[str, Path], records: dict[str, Record]) -> list[Response]:
    """Join the grading key and sheets to the records after checking their consistency."""
    keys = _rows_by_id(inputs["grading_key"], KEY_FIELDS)
    pass1 = _rows_by_id(inputs["grading_pass1"], SHEET_FIELDS)
    pass2 = _rows_by_id(inputs["grading_pass2"], SHEET_FIELDS)
    llm = _rows_by_id(inputs["llm_labels"], LLM_FIELDS)
    if not set(keys) == set(pass1) == set(pass2) == set(llm):
        raise ValueError("grading key, sheets, and LLM labels list different grading ids")

    responses: list[Response] = []
    graded: set[tuple[str, str]] = set()
    for grading_id in sorted(keys):
        key = keys[grading_id]
        record = records.get(key["open_find_run_id"])
        if record is None:
            raise ValueError(f"{grading_id}: unknown run {key['open_find_run_id']}")
        expected_key = {
            "grading_id": grading_id,
            "open_find_run_id": record.run_id,
            "condition": record.condition,
            "finder_id": record.finder_id,
            "trial_id": record.trial_id,
            "seed": record.seed,
            "source_build_run_id": record.source_build_run_id,
        }
        if key != expected_key:
            raise ValueError(f"{grading_id}: grading key does not match {record.run_id}")
        first, second = pass1[grading_id], pass2[grading_id]
        question_id = first["question_id"]
        if question_id not in record.answers:
            raise ValueError(f"{grading_id}: {record.run_id} has no answer to {question_id}")
        content = sheet_content(record, question_id)
        for name in SHEET_CONTENT_FIELDS:
            if first[name] != content[name] or second[name] != content[name]:
                raise ValueError(f"{grading_id}: sheet column {name} differs from the record")
        if first["shuffle_index"] != second["shuffle_index"]:
            raise ValueError(f"{grading_id}: pass-1 and pass-2 sheets differ in row order")
        if first["evidence_sufficient"]:
            raise ValueError(f"{grading_id}: the pass-1 sheet carries an evidence label")
        _require_label(grading_id, first["claim_correct"], CLAIM_LABELS)
        _require_label(grading_id, second["claim_correct"], CLAIM_LABELS)
        _require_label(grading_id, second["evidence_sufficient"], EVIDENCE_LABELS)
        _require_label(grading_id, llm[grading_id]["claim_correct"], CLAIM_LABELS)
        _require_label(grading_id, llm[grading_id]["evidence_sufficient"], EVIDENCE_LABELS)
        graded.add((record.run_id, question_id))
        responses.append(
            Response(
                grading_id=grading_id,
                run_id=record.run_id,
                condition=record.condition,
                finder_id=record.finder_id,
                trial_id=record.trial_id,
                task_id=record.task_id,
                question_id=question_id,
                claim_label=first["claim_correct"],
                pass2_claim_label=second["claim_correct"],
                evidence_label=second["evidence_sufficient"],
                cites_evidence=bool(first["evidence_files"]),
                llm_claim_label=llm[grading_id]["claim_correct"],
                llm_evidence_label=llm[grading_id]["evidence_sufficient"],
                has_note=bool(second["grader_note"]),
            )
        )
    answered = {
        (record.run_id, question) for record in records.values() for question in record.answers
    }
    if graded != answered or len(responses) != len(answered):
        raise ValueError("the grading sheet does not cover every recorded answer exactly once")
    order = sorted(
        keys, key=lambda gid: shuffle_hash(keys[gid]["open_find_run_id"], pass1[gid]["question_id"])
    )
    if [int(pass1[gid]["shuffle_index"]) for gid in order] != list(range(1, len(order) + 1)):
        raise ValueError("grading-sheet row order does not follow the frozen shuffle rule")
    return responses


def multiple_choice_arms(
    answer_rows: Iterable[dict[str, str]],
    find_rows: Iterable[dict[str, str]],
    excluded_rows: Iterable[dict[str, str]],
    *,
    artifacts: dict[str, str],
    pairs: Iterable[Pair],
) -> MultipleChoice:
    """The released multiple-choice answers of the same finders, artifacts, and questions."""
    wanted = set(pairs)
    present: list[dict[str, str]] = []
    only: list[dict[str, str]] = []
    for row in answer_rows:
        if row["finder_id"] not in FINDERS or (row["task_id"], row["question_id"]) not in wanted:
            continue
        panels = set(row["panel_ids"].split("|"))
        if QUESTION_ONLY_PANEL_ID in panels:
            only.append(row)
        elif PAPER_FINAL_PANEL_ID in panels and row["build_run_id"] == artifacts[row["task_id"]]:
            present.append(row)
    expected = len(FINDERS) * len(TRIALS) * len(wanted)
    for name, rows in (("artifact-present", present), ("question-only", only)):
        cells = {
            (row["finder_id"], row["task_id"], row["finder_trial_id"], row["question_id"])
            for row in rows
        }
        if len(rows) != expected or len(cells) != expected:
            raise ValueError(
                f"{name} multiple-choice arm has {len(rows)} rows, expected {expected}"
            )
    present_runs = tuple(sorted({row["find_run_id"] for row in present}))
    only_runs = tuple(sorted({row["find_run_id"] for row in only}))
    created = {row["find_run_id"]: row["find_created_at"] for row in find_rows}
    excluded = {row["find_run_id"] for row in excluded_rows}
    return MultipleChoice(
        artifact_present=Rate.of(_as_bool(row["is_correct"]) for row in present),
        question_only=Rate.of(_as_bool(row["is_correct"]) for row in only),
        artifact_present_runs=present_runs,
        question_only_runs=only_runs,
        question_only_runs_excluded=len(excluded & set(only_runs)),
        run_dates=tuple(sorted({created[run][:10] for run in (*present_runs, *only_runs)})),
    )


def still_low_prior(rows: Iterable[dict[str, str]], pairs: Sequence[Pair]) -> tuple[Pair, ...]:
    """Option-free questions that stay below 90% agreement on the corrected controls."""
    by_pair = {(row["task_id"], row["question_id"]): row for row in rows}
    missing = [f"{task}:{question}" for task, question in pairs if (task, question) not in by_pair]
    if missing:
        raise ValueError(f"control low-prior table lacks {', '.join(missing)}")
    return tuple(pair for pair in pairs if _as_bool(by_pair[pair]["still_low_prior"]))


def label_overlay(inputs: dict[str, Path]) -> LabelOverlay:
    """Build the observed-error and adjudicated label overlays on the v1.1 labels."""
    released_rows = _apply_label_corrections(
        _read_csv(inputs["final_labels"]), _read_csv(inputs["label_corrections"])
    )
    observed = (
        *sensitivity.PILOT_CORRECTIONS,
        *sensitivity._load_spot_audit_corrections(
            minidb_rows=_read_csv(inputs["spot_audit_minidb"]),
            nanoweb_rows=_read_csv(inputs["spot_audit_nanoweb"]),
            sample_manifest_rows=_read_csv(inputs["spot_audit_sample_manifest"]),
        ),
    )
    flag_rows = _read_csv(inputs["full_audit_disagreements"])
    full_audit = sensitivity._load_full_audit_corrections(flag_rows)
    author = sensitivity._load_author_decisions(
        _read_csv(inputs["author_decisions"]), disagreement_rows=flag_rows
    )
    author_adjudicated, _ = sensitivity._author_adjudicated_corrections(
        observed=observed, full_audit=full_audit, decisions=author
    )
    model = sensitivity._load_model_assisted_decisions(
        _read_csv(inputs["model_assisted_decisions"]),
        disagreement_rows=flag_rows,
        author_decisions=author,
    )
    adjudicated = sensitivity._fully_adjudicated_corrections(
        observed=observed,
        author_adjudicated=author_adjudicated,
        full_audit=full_audit,
        decisions=model,
    )
    corrections = {OBSERVED_ERRORS: tuple(observed), ADJUDICATED: tuple(adjudicated)}
    scenarios = {
        scenario: _labels(
            sensitivity._apply_corrections(
                released_rows, corrections=rows, label_source=f"{ANALYSIS_ID}_{scenario}"
            )
        )
        for scenario, rows in corrections.items()
    }
    decisions = {decision.key: "author" for decision in author}
    decisions.update({decision.key: f"model-assisted, {decision.review}" for decision in model})
    return LabelOverlay(
        released=_labels(released_rows),
        scenarios=scenarios,
        corrections={scenario: len(rows) for scenario, rows in corrections.items()},
        flags={
            (row["task_id"], row["artifact_id"], row["question_id"]): row["disagreement_basis"]
            for row in flag_rows
        },
        decisions=decisions,
    )


def pair_labels(
    overlay: LabelOverlay, pairs: Sequence[Pair], artifacts: dict[str, str]
) -> list[PairLabel]:
    rows: list[PairLabel] = []
    for task, question in pairs:
        key = (task, artifacts[task], question)
        if key not in overlay.released:
            raise ValueError(f"no implementation label for {key}")
        rows.append(
            PairLabel(
                task_id=task,
                question_id=question,
                artifact_id=artifacts[task],
                released=overlay.released[key],
                flag=overlay.flags.get(key, ""),
                decision=overlay.decisions.get(key, ""),
                scenario_labels={
                    scenario: overlay.scenarios[scenario][key] for scenario in LABEL_SCENARIOS
                },
            )
        )
    return rows


def artifact_labels(overlay: LabelOverlay, artifacts: dict[str, str]) -> list[ArtifactLabels]:
    rows: list[ArtifactLabels] = []
    for task in TASK_IDS:
        keys = [key for key in overlay.released if key[0] == task and key[1] == artifacts[task]]
        rows.append(
            ArtifactLabels(
                task_id=task,
                artifact_id=artifacts[task],
                pairs=len(keys),
                released_gold=sum(overlay.released[key] == IMPLEMENTED_GOLD for key in keys),
                flagged=sum(key in overlay.flags for key in keys),
                adjudicated_gold=sum(
                    overlay.scenarios[ADJUDICATED][key] == IMPLEMENTED_GOLD for key in keys
                ),
            )
        )
    return rows


# --------------------------------------------------------------------------- counts


def compare(
    responses: Iterable[Response], scope: str, keep: Callable[[Response], bool] = lambda _: True
) -> Comparison:
    rows = [row for row in responses if keep(row)]
    return Comparison(
        scope=scope,
        artifact_open=Rate.of(row.correct for row in rows if row.condition == ARTIFACT_OPEN),
        question_only_open=Rate.of(
            row.correct for row in rows if row.condition == QUESTION_ONLY_OPEN
        ),
    )


def comparisons(
    responses: list[Response], pairs: Sequence[Pair], low_prior: Sequence[Pair]
) -> dict[str, Comparison]:
    scopes = [compare(responses, "all")]
    scopes.extend(
        compare(responses, f"finder:{finder}", lambda row, f=finder: row.finder_id == f)
        for finder in FINDERS
    )
    scopes.extend(
        compare(responses, f"task:{task}", lambda row, t=task: row.task_id == t)
        for task in TASK_IDS
    )
    scopes.extend(
        compare(
            responses, f"question:{task}:{question}", lambda row, p=(task, question): row.pair == p
        )
        for task, question in pairs
    )
    still = set(low_prior)
    scopes.append(compare(responses, "low_prior_corrected", lambda row: row.pair in still))
    return {scope.scope: scope for scope in scopes}


def sensitivities(responses: list[Response], labels: Sequence[PairLabel]) -> list[Sensitivity]:
    rows: list[Sensitivity] = []
    for scenario in LABEL_SCENARIOS:
        changed = tuple(label.pair for label in labels if label.changed(scenario))
        dropped = set(changed)
        rows.append(
            Sensitivity(
                scenario=scenario,
                changed_pairs=changed,
                comparison=compare(
                    responses, f"excluding:{scenario}", lambda row: row.pair not in dropped
                ),
            )
        )
    return rows


def grading_summary(responses: list[Response]) -> Grading:
    artifact_rows = [row for row in responses if row.condition == ARTIFACT_OPEN]
    evidence_labels = {
        condition: dict(
            sorted(
                Counter(
                    row.evidence_label for row in responses if row.condition == condition
                ).items()
            )
        )
        for condition in CONDITIONS
    }
    return Grading(
        correctness_unchanged=Rate.of(
            row.claim_label == row.pass2_claim_label for row in responses
        ),
        evidence_labels=evidence_labels,
        artifact_sufficient=Rate.of(row.evidence_label == SUFFICIENT for row in artifact_rows),
        sufficient_among_correct=Rate.of(
            row.evidence_label == SUFFICIENT for row in artifact_rows if row.correct
        ),
        cited_evidence_rows={
            condition: Rate.of(
                row.cites_evidence for row in responses if row.condition == condition
            )
            for condition in CONDITIONS
        },
        grader_notes=sum(row.has_note for row in responses),
        llm_correctness_agreement=Rate.of(
            row.llm_claim_label == row.claim_label for row in responses
        ),
        llm_correct_author_incorrect=sum(
            row.llm_claim_label == CORRECT and row.claim_label != CORRECT for row in responses
        ),
        llm_incorrect_author_correct=sum(
            row.llm_claim_label != CORRECT and row.claim_label == CORRECT for row in responses
        ),
        llm_evidence_agreement=Rate.of(
            row.llm_evidence_label == row.evidence_label for row in responses
        ),
    )


# --------------------------------------------------------------------------- analysis


def analyze(p: Paths, *, input_dir: Path) -> Analysis:
    inputs = analysis_inputs(p, input_dir=input_dir)
    record_paths = sorted((input_dir / RECORDS_DIR).glob("openfind-*.json"))
    missing = [_display_path(path) for path in inputs.values() if not path.exists()]
    if not record_paths:
        missing.append(_display_path(input_dir / RECORDS_DIR / "openfind-*.json"))
    if missing:
        raise FileNotFoundError(f"missing inputs: {', '.join(missing)}")

    records = load_records(record_paths)
    questions = check_matrix(records)
    artifacts = used_artifacts(records)
    answer_rows = _read_csv(inputs["question_answers"])
    find_rows = _read_csv(inputs["find_runs"])
    question_types = {
        (row["task_id"], row["question_id"]): row["question_type"] for row in answer_rows
    }
    frozen = tuple(
        sorted(
            tuple(value.split(":", 1))
            for value in _read_json(inputs["low_prior_manifest"])["question_subset"]["questions"]
        )
    )
    excluded_questions = check_question_selection(frozen, question_types, questions)
    rule = artifact_rule(
        _read_csv(inputs["build_runs"]),
        _read_csv(inputs["final_labels"]),
        finder_families={row["finder_family"] for row in find_rows if row["finder_id"] in FINDERS},
        used=artifacts,
    )
    if not rule.reproduced:
        raise ValueError(f"artifact rule selects {rule.selected}, records use {rule.used}")

    responses = load_responses(inputs, records)
    pairs = tuple((task, question) for task in TASK_IDS for question in questions[task])
    low_prior = still_low_prior(_read_csv(inputs["control_low_prior"]), pairs)
    overlay = label_overlay(inputs)
    labels = pair_labels(overlay, pairs, artifacts)
    grading = grading_summary(responses)

    analysis = Analysis(
        inputs=inputs,
        record_paths=record_paths,
        generated_at=_read_json(inputs["table_manifest"]).get("generated_at"),
        records=records,
        questions=questions,
        frozen_low_prior=frozen,
        excluded_questions=excluded_questions,
        rule=rule,
        responses=responses,
        comparisons=comparisons(responses, pairs, low_prior),
        multiple_choice=multiple_choice_arms(
            answer_rows,
            find_rows,
            _read_csv(inputs["control_exclusions"]),
            artifacts=artifacts,
            pairs=pairs,
        ),
        still_low_prior=low_prior,
        pair_labels=labels,
        sensitivities=sensitivities(responses, labels),
        artifacts=artifact_labels(overlay, artifacts),
        overlay_corrections=overlay.corrections,
        grading=grading,
        printed=[],
    )
    return replace(analysis, printed=printed_values(analysis))


# --------------------------------------------------------------------------- printed values


def _pair_text(pairs: Iterable[Pair]) -> str:
    return ", ".join(question for _, question in pairs)


def _token_text(tokens: int) -> str:
    return f"{tokens / 1_000_000:.2f}M"


def printed_values(analysis: Analysis) -> list[PrintedValue]:
    """Every option-free value the paper prints, with the input fields it comes from."""
    values: list[PrintedValue] = []
    graded = (
        "grading_sheet.pass1.csv:claim_correct joined on grading_id with grading_key.csv:condition"
    )

    def text(value_id: str, printed: str, source: str, exact: str = "") -> None:
        values.append(PrintedValue(value_id, printed, exact, source))

    def count(value_id: str, rate: Rate, source: str) -> None:
        exact = f"{rate.count} = {_exact_percent(rate.value)}%"
        values.append(PrintedValue(value_id, rate.count, exact, source))

    def rate_pct(value_id: str, rate: Rate, source: str) -> None:
        exact = f"{rate.count} = {_exact_percent(rate.value)}%"
        values.append(
            PrintedValue(
                value_id, percent(rate.value), exact, source, is_half_tie(100 * rate.value)
            )
        )

    def comparison(prefix: str, value: Comparison, source: str, *, pct: bool = False) -> None:
        count(f"{prefix}.artifact_open", value.artifact_open, source)
        count(f"{prefix}.question_only_open", value.question_only_open, source)
        if pct:
            rate_pct(f"{prefix}.artifact_open_pct", value.artifact_open, source)
            rate_pct(f"{prefix}.question_only_open_pct", value.question_only_open, source)
        values.append(
            PrintedValue(
                f"{prefix}.difference_pp",
                signed_points(value.difference),
                f"{_exact_points(value.difference)} pp",
                source,
                is_half_tie(100 * value.difference),
            )
        )

    records = analysis.records
    text(
        "design.questions",
        f"{len(analysis.pairs)} of {len(analysis.frozen_low_prior)}",
        "low_prior/manifest.json:question_subset.questions minus question_answers.csv:"
        "question_type == negative_contrast; records:question_ids",
    )
    text(
        "design.excluded_questions",
        _pair_text(analysis.excluded_questions),
        "question_answers.csv:question_type == negative_contrast",
    )
    text("design.sessions", str(len(records)), "records/openfind-*.json")
    text("design.responses", str(len(analysis.responses)), "grading_key.csv rows")
    text("design.trials", str(len(TRIALS)), "records:manifest.trial_id")
    text("design.finders", ", ".join(FINDERS), "records:manifest.agent.id")
    for task in TASK_IDS:
        text(
            f"design.artifact.{task}",
            analysis.rule.used[task],
            "records:source_build_run_id; replayed artifact rule on build_runs.csv and "
            "artifact_conformance_final.csv",
        )

    comparison("option_free", analysis.comparisons["all"], graded, pct=True)
    mcq = analysis.multiple_choice
    mcq_source = (
        "question_answers.csv:is_correct (panel paper_final_scratch on the two artifacts, "
        "or panel question_only), same finders and questions"
    )
    count("mcq.artifact_present", mcq.artifact_present, mcq_source)
    count("mcq.question_only", mcq.question_only, mcq_source)
    text(
        "mcq.question_only_runs_excluded",
        str(mcq.question_only_runs_excluded),
        "excluded_question_only_runs.csv:find_run_id",
    )
    for finder in FINDERS:
        comparison(f"finder.{finder}", analysis.comparisons[f"finder:{finder}"], graded)
    for task in TASK_IDS:
        comparison(f"task.{task}", analysis.comparisons[f"task:{task}"], graded)
    for task, question in analysis.pairs:
        value = analysis.comparisons[f"question:{task}:{question}"]
        count(f"question.{question}.artifact_open", value.artifact_open, graded)
        count(f"question.{question}.question_only_open", value.question_only_open, graded)

    low_prior_source = "control_exclusions/low_prior_questions.csv:still_low_prior"
    text(
        "low_prior.still_low_prior",
        f"{len(analysis.still_low_prior)} of {len(analysis.pairs)}",
        low_prior_source,
    )
    text("low_prior.questions", _pair_text(analysis.still_low_prior), low_prior_source)
    comparison(
        "low_prior", analysis.comparisons["low_prior_corrected"], f"{graded}; {low_prior_source}"
    )

    for row in analysis.sensitivities:
        source = f"{graded}; pairs whose label changes under {SCENARIO_DESCRIPTIONS[row.scenario]}"
        text(f"sensitivity.{row.scenario}.pairs", str(len(row.changed_pairs)), source)
        text(f"sensitivity.{row.scenario}.questions", _pair_text(row.changed_pairs), source)
        comparison(f"sensitivity.{row.scenario}", row.comparison, source)

    label_source = (
        "artifact_conformance_final.csv + label_corrections_v1_1.csv:final_label; "
        "disagreements.csv; author_decisions.csv; model_assisted_decisions.csv"
    )
    for row in analysis.artifacts:
        text(f"artifact.{row.task_id}.full_audit_flags", f"{row.flagged}/{row.pairs}", label_source)
        text(
            f"artifact.{row.task_id}.released_implemented_gold",
            f"{row.released_gold}/{row.pairs}",
            label_source,
        )
        text(
            f"artifact.{row.task_id}.adjudicated_implemented_gold",
            f"{row.adjudicated_gold}/{row.pairs}",
            label_source,
        )

    grading = analysis.grading
    count(
        "grading.correctness_unchanged",
        grading.correctness_unchanged,
        "grading_sheet.pass1.csv:claim_correct == grading_sheet.pass2.csv:claim_correct",
    )
    count(
        "grading.artifact_open_evidence_sufficient",
        grading.artifact_sufficient,
        "grading_sheet.pass2.csv:evidence_sufficient on artifact_open rows",
    )
    count(
        "grading.sufficient_among_correct",
        grading.sufficient_among_correct,
        "grading_sheet.pass2.csv:evidence_sufficient on correct artifact_open rows",
    )
    for condition in CONDITIONS:
        count(
            f"grading.pass1_evidence_rows.{condition}",
            grading.cited_evidence_rows[condition],
            "grading_sheet.pass1.csv:evidence_files non-empty",
        )
    text(
        "grading.question_only_no_evidence_cited",
        str(grading.evidence_labels[QUESTION_ONLY_OPEN].get("no_evidence_cited", 0)),
        "grading_sheet.pass2.csv:evidence_sufficient on question_only_open rows",
    )
    llm_source = "llm_prelabels.csv:claim_correct vs grading_sheet.pass1.csv:claim_correct"
    count("grading.llm_correctness_agreement", grading.llm_correctness_agreement, llm_source)
    text(
        "grading.llm_disagreements",
        str(grading.llm_correct_author_incorrect + grading.llm_incorrect_author_correct),
        llm_source,
    )
    text(
        "grading.llm_disagreements_author_stricter",
        str(grading.llm_correct_author_incorrect),
        llm_source,
    )
    count(
        "grading.llm_evidence_agreement",
        grading.llm_evidence_agreement,
        "llm_prelabels.csv:evidence_sufficient vs grading_sheet.pass2.csv:evidence_sufficient",
    )

    tokens = sum(record.vendor_total_tokens for record in records.values())
    text(
        "compute.vendor_tokens",
        _token_text(tokens),
        "records:usage.vendor_usage.total_tokens",
        f"{tokens}",
    )
    text(
        "dates.option_free_sessions",
        ", ".join(sorted({record.created_at[:10] for record in records.values()})),
        "records:manifest.created_at (UTC)",
    )
    text(
        "dates.mcq_runs",
        ", ".join(mcq.run_dates),
        "find_runs.csv:find_created_at (UTC)",
    )
    text(
        "dates.option_free_sessions_month",
        _month_text(record.created_at for record in records.values()),
        "records:manifest.created_at (UTC)",
    )
    text(
        "dates.mcq_runs_month",
        _month_text(mcq.run_dates),
        "find_runs.csv:find_created_at (UTC)",
    )
    return values


def _month_text(timestamps: Iterable[str]) -> str:
    """Month and year of ISO timestamps or dates, e.g. ``July 2026``; one month only."""
    months = {stamp[:7] for stamp in timestamps}
    if len(months) != 1:
        raise ValueError(f"expected one calendar month, found {sorted(months)}")
    year, month = next(iter(months)).split("-")
    return f"{MONTH_NAMES[int(month) - 1]} {year}"


# --------------------------------------------------------------------------- outputs


def write_outputs(analysis: Analysis, out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "summary.json").write_text(
        json.dumps(_summary(analysis), indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    (out_dir / "report.md").write_text(_report(analysis), encoding="utf-8")


def _rate_json(rate: Rate) -> dict[str, Any]:
    return {"correct": rate.correct, "total": rate.total, "pct": percent(rate.value)}


def _comparison_json(value: Comparison) -> dict[str, Any]:
    return {
        "artifact_open": _rate_json(value.artifact_open),
        "question_only_open": _rate_json(value.question_only_open),
        "difference_pp": signed_points(value.difference),
    }


def _summary(analysis: Analysis) -> dict[str, Any]:
    records = analysis.records
    grading = analysis.grading
    mcq = analysis.multiple_choice
    input_paths = [*analysis.inputs.values(), *analysis.record_paths]
    tokens_by_finder = {
        finder: sum(r.vendor_total_tokens for r in records.values() if r.finder_id == finder)
        for finder in FINDERS
    }
    return {
        "analysis_id": ANALYSIS_ID,
        "analysis_script": "scripts/analyze_option_free_check.py",
        "table_manifest_generated_at": analysis.generated_at,
        "inputs": {name: _display_path(path) for name, path in analysis.inputs.items()},
        "records": [_display_path(path) for path in analysis.record_paths],
        "input_hashes": {_display_path(path): _sha256(path) for path in input_paths},
        "design": {
            "questions": {task: list(ids) for task, ids in analysis.questions.items()},
            "frozen_low_prior_questions": [f"{t}:{q}" for t, q in analysis.frozen_low_prior],
            "excluded_negative_contrast": [f"{t}:{q}" for t, q in analysis.excluded_questions],
            "conditions": list(CONDITIONS),
            "finders": list(FINDERS),
            "trials": len(TRIALS),
            "sessions": len(records),
            "responses": len(analysis.responses),
            "artifacts": analysis.rule.used,
        },
        "checks": {
            "records_drop_trace_path": True,
            "grading_sheets_match_records": True,
            "shuffle_order_reproduced": True,
            "shuffle_rule": f"sort by sha256('{SHUFFLE_SEED}|<open_find_run_id>|<question_id>')",
            "questions_match_frozen_low_prior_without_negative_contrast": True,
            "artifact_rule_reproduced": analysis.rule.reproduced,
        },
        "artifact_rule": {
            "rule": (
                "compile_pass, implemented_gold on every question under the released v1.0 "
                "labels, builder_family outside the finder families, first build_run_id"
            ),
            "finder_families": list(analysis.rule.finder_families),
            "eligible": analysis.rule.eligible,
            "selected": analysis.rule.selected,
        },
        "option_free": {
            scope: _comparison_json(value) for scope, value in analysis.comparisons.items()
        },
        "multiple_choice": {
            "artifact_present": _rate_json(mcq.artifact_present),
            "question_only": _rate_json(mcq.question_only),
            "artifact_present_runs": list(mcq.artifact_present_runs),
            "question_only_runs": list(mcq.question_only_runs),
            "question_only_runs_excluded_by_control_exclusions": mcq.question_only_runs_excluded,
            "run_dates_utc": list(mcq.run_dates),
        },
        "low_prior_corrected_controls": {
            "still_low_prior": [f"{t}:{q}" for t, q in analysis.still_low_prior],
            "still_low_prior_count": len(analysis.still_low_prior),
            "option_free_questions": len(analysis.pairs),
        },
        "label_sensitivity": {
            "label_base": "v1.0 labels with label_corrections_v1_1.csv",
            "corrected_pairs": analysis.overlay_corrections,
            "pairs": [
                {
                    "task_id": row.task_id,
                    "question_id": row.question_id,
                    "artifact_id": row.artifact_id,
                    "released_label": row.released,
                    "full_audit_flag": row.flag,
                    "decision": row.decision,
                    **{
                        f"{scenario}_label": row.scenario_labels[scenario]
                        for scenario in LABEL_SCENARIOS
                    },
                }
                for row in analysis.pair_labels
            ],
            "scenarios": {
                row.scenario: {
                    "description": SCENARIO_DESCRIPTIONS[row.scenario],
                    "excluded_pairs": [f"{t}:{q}" for t, q in row.changed_pairs],
                    **_comparison_json(row.comparison),
                }
                for row in analysis.sensitivities
            },
            "artifacts": [
                {
                    "task_id": row.task_id,
                    "artifact_id": row.artifact_id,
                    "pairs": row.pairs,
                    "released_implemented_gold": row.released_gold,
                    "full_audit_flags": row.flagged,
                    "adjudicated_implemented_gold": row.adjudicated_gold,
                }
                for row in analysis.artifacts
            ],
        },
        "grading": {
            "correctness_unchanged_pass1_to_pass2": _rate_json(grading.correctness_unchanged),
            "evidence_labels_pass2": grading.evidence_labels,
            "artifact_open_evidence_sufficient": _rate_json(grading.artifact_sufficient),
            "sufficient_among_correct_artifact_open": _rate_json(grading.sufficient_among_correct),
            "pass1_rows_with_evidence_citations": {
                condition: _rate_json(rate)
                for condition, rate in grading.cited_evidence_rows.items()
            },
            "rows_with_grader_note": grading.grader_notes,
            "llm_correctness_agreement": _rate_json(grading.llm_correctness_agreement),
            "llm_correct_author_incorrect": grading.llm_correct_author_incorrect,
            "llm_incorrect_author_correct": grading.llm_incorrect_author_correct,
            "llm_evidence_agreement": _rate_json(grading.llm_evidence_agreement),
        },
        "compute": {
            "sessions": len(records),
            "vendor_total_tokens": sum(tokens_by_finder.values()),
            "vendor_total_tokens_by_finder": tokens_by_finder,
            "wall_clock_seconds": round(sum(r.wall_clock_seconds for r in records.values()), 1),
            "session_dates_utc": sorted({r.created_at[:10] for r in records.values()}),
            "first_session_created_at": min(r.created_at for r in records.values()),
        },
        "printed_values": [
            {
                "value_id": value.value_id,
                "printed": value.printed,
                "exact": value.exact,
                "source": value.source,
                "half_tie": int(value.half_tie),
            }
            for value in analysis.printed
        ],
        "outputs": ["summary.json", "report.md"],
    }


def _report(analysis: Analysis) -> str:
    overall = analysis.comparisons["all"]
    mcq = analysis.multiple_choice
    grading = analysis.grading
    lines = [
        "# Option-free check",
        "",
        "Generated by `scripts/analyze_option_free_check.py`; do not edit by hand. See",
        "`README.md` for the design and the grading procedure.",
        "",
        f"{len(analysis.records)} sessions ({len(FINDERS)} finders x {len(CONDITIONS)} conditions x "
        f"{len(TRIALS)} trials x {len(TASK_IDS)} tasks) answered {len(analysis.pairs)} of the "
        f"{len(analysis.frozen_low_prior)} frozen low-prior questions as free-text claims "
        f"({len(analysis.responses)} responses). The other "
        f"{len(analysis.excluded_questions)} ({_pair_text(analysis.excluded_questions)}) are "
        "negative-contrast items that require visible choices.",
        "",
        "## Checks",
        "",
        "- Grading sheets reproduce the recorded claims, evidence, and rationales: yes",
        f"- Row order follows `sha256('{SHUFFLE_SEED}|<run>|<question>')`: yes",
        "- Records ask the frozen low-prior questions without the negative-contrast items: yes",
        "- Artifact rule selects the artifacts the sessions used: "
        + ("yes" if analysis.rule.reproduced else "NO"),
        f"- Pass-1 correctness labels unchanged in pass 2: {grading.correctness_unchanged.count}",
        "",
        "## Correct claims",
        "",
        "| scope | artifact present | question only | difference (pp) |",
        "| --- | ---: | ---: | ---: |",
    ]

    def row(label: str, value: Comparison) -> str:
        return (
            f"| {label} | {value.artifact_open.count} ({percent(value.artifact_open.value)}%) | "
            f"{value.question_only_open.count} ({percent(value.question_only_open.value)}%) | "
            f"{signed_points(value.difference)} |"
        )

    lines.append(row("all", overall))
    for finder in FINDERS:
        lines.append(row(f"`{finder}`", analysis.comparisons[f"finder:{finder}"]))
    for task in TASK_IDS:
        lines.append(row(f"`{task}`", analysis.comparisons[f"task:{task}"]))
    lines.append(
        row(
            f"still low-prior on corrected controls ({len(analysis.still_low_prior)} questions)",
            analysis.comparisons["low_prior_corrected"],
        )
    )
    for sensitivity_row in analysis.sensitivities:
        lines.append(
            row(
                f"excluding {len(sensitivity_row.changed_pairs)} pairs relabeled under "
                f"{SCENARIO_DESCRIPTIONS[sensitivity_row.scenario]}",
                sensitivity_row.comparison,
            )
        )
    lines.extend(
        [
            "",
            "Multiple-choice answers of the same finders on the same artifacts and questions "
            f"(released runs, {', '.join(mcq.run_dates)}): artifact present "
            f"{mcq.artifact_present.count}, question only {mcq.question_only.count}. "
            f"{mcq.question_only_runs_excluded} of the {len(mcq.question_only_runs)} "
            "question-only runs are in the control exclusion list.",
            "",
            "## Per question",
            "",
            "| question | artifact present | question only | still low-prior | "
            "released label | adjudicated label |",
            "| --- | ---: | ---: | :---: | --- | --- |",
        ]
    )
    labels = {label.pair: label for label in analysis.pair_labels}
    still = set(analysis.still_low_prior)
    for task, question in analysis.pairs:
        value = analysis.comparisons[f"question:{task}:{question}"]
        label = labels[(task, question)]
        lines.append(
            f"| `{task}:{question}` | {value.artifact_open.count} | "
            f"{value.question_only_open.count} | {'yes' if (task, question) in still else 'no'} | "
            f"{label.released} | {label.scenario_labels[ADJUDICATED]} |"
        )
    lines.extend(
        [
            "",
            "## Implementation labels of the option-free pairs",
            "",
            "The released labels are the v1.0 labels with `label_corrections_v1_1.csv`. "
            "Corrected pairs per overlay: "
            + ", ".join(
                f"{SCENARIO_DESCRIPTIONS[name]} {count}"
                for name, count in analysis.overlay_corrections.items()
            )
            + ".",
            "",
            "| question | full-audit flag | decision | observed-error label | adjudicated label |",
            "| --- | --- | --- | --- | --- |",
        ]
    )
    for label in analysis.pair_labels:
        lines.append(
            f"| `{label.task_id}:{label.question_id}` | {label.flag or '-'} | "
            f"{label.decision or '-'} | {label.scenario_labels[OBSERVED_ERRORS]} | "
            f"{label.scenario_labels[ADJUDICATED]} |"
        )
    lines.extend(
        [
            "",
            "| artifact | implemented-gold (released) | full-audit flags | "
            "implemented-gold (adjudicated) |",
            "| --- | ---: | ---: | ---: |",
        ]
    )
    for artifact in analysis.artifacts:
        lines.append(
            f"| `{artifact.artifact_id}` | {artifact.released_gold}/{artifact.pairs} | "
            f"{artifact.flagged}/{artifact.pairs} | {artifact.adjudicated_gold}/{artifact.pairs} |"
        )
    lines.extend(
        [
            "",
            "## Grading",
            "",
            "- Artifact-present claims graded `sufficient` on evidence: "
            f"{grading.artifact_sufficient.count} (correct claims: "
            f"{grading.sufficient_among_correct.count})",
            "- Pass-1 sheet rows with evidence citations: "
            + ", ".join(
                f"{condition} {grading.cited_evidence_rows[condition].count}"
                for condition in CONDITIONS
            ),
            "- Pass-2 evidence labels: "
            + "; ".join(
                f"{condition} " + ", ".join(f"{name} {count}" for name, count in counts.items())
                for condition, counts in grading.evidence_labels.items()
            ),
            f"- Rows with a grader note: {grading.grader_notes}",
            "- LLM labels agree with the author on correctness: "
            f"{grading.llm_correctness_agreement.count} "
            f"({grading.llm_correct_author_incorrect} LLM correct / author incorrect, "
            f"{grading.llm_incorrect_author_correct} LLM incorrect / author correct); "
            f"on evidence: {grading.llm_evidence_agreement.count}",
            "",
            "## Printed values",
            "",
            "| value | printed | exact | source |",
            "| --- | ---: | --- | --- |",
        ]
    )
    for value in analysis.printed:
        exact = value.exact + (" (tie)" if value.half_tie else "")
        lines.append(f"| `{value.value_id}` | {value.printed} | {exact} | {value.source} |")
    lines.append("")
    return "\n".join(lines)


# --------------------------------------------------------------------------- io


def _labels(rows: Iterable[dict[str, str]]) -> dict[LabelKey, str]:
    return {_artifact_question_key(row): row["final_label"] for row in rows}


def _require_label(grading_id: str, value: str, allowed: Sequence[str]) -> None:
    if value not in allowed:
        raise ValueError(f"{grading_id}: label {value!r} is not one of {', '.join(allowed)}")


def _rows_by_id(path: Path, fields: Sequence[str]) -> dict[str, dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        if tuple(reader.fieldnames or ()) != tuple(fields):
            raise ValueError(f"{path.name}: expected columns {', '.join(fields)}")
        rows: dict[str, dict[str, str]] = {}
        for row in reader:
            if row["grading_id"] in rows:
                raise ValueError(f"{path.name}: {row['grading_id']} listed twice")
            rows[row["grading_id"]] = row
    return rows


def _exact_percent(value: Fraction) -> str:
    return f"{float(100 * value):.4f}"


def _exact_points(value: Fraction) -> str:
    return f"{float(100 * value):+.4f}"


def _read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle))


def _read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def _as_bool(value: str) -> bool:
    return value.strip().lower() in {"1", "true", "yes"}


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
