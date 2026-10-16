"""Create a model-consensus triage table for artifact conformance annotation.

This analysis is a pre-audit helper, not a conformance oracle. It uses selected
high-performing finder records to identify artifact-question pairs where finder
answers strongly agree on the gold option or on the same non-gold option. Rows
without consensus are queued for manual conformance labeling.
"""

# ruff: noqa: E402

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import yaml

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src"
if str(SRC) not in sys.path:
    sys.path.insert(0, str(SRC))

from bnf.config import paths


DEFAULT_BASE_MODELS = ("codex_gpt5_5", "claude_code_opus_4_7")
DEFAULT_FINDER_EFFORTS = ("high",)
FINAL_CONFORMANCE_LABELS = (
    "implemented_gold",
    "implemented_non_gold",
    "absent_or_ambiguous",
    "documented_only",
    "not_applicable_or_invalid",
)
CONSENSUS_FIELD_ORDER = [
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
    "consensus_status",
    "preliminary_label",
    "consensus_canonical_option",
    "consensus_text",
    "consensus_matches_gold",
    "consensus_reason",
    "model_vote_summary",
    "selected_answer_rows",
    "selected_base_models",
    "reviewer_action",
    "allowed_final_labels",
    "source_requirement_id",
    "source_intention_id",
    "artifact_exists",
    "rust_files",
    "rust_loc",
]
VOTE_FIELD_ORDER = [
    "task_id",
    "artifact_id",
    "builder_id",
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
    "base_model",
    "finder_id",
    "finder_base_model",
    "vote_canonical_option",
    "vote_text",
    "stable",
    "agreement",
    "top_count",
    "observations",
    "selected_options",
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
    "manual_review_required",
    "manual_review_rate",
]
MANUAL_ANNOTATION_FIELD_ORDER = [
    "task_id",
    "artifact_id",
    "artifact_path",
    "builder_id",
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
    "consensus_reason",
    "model_vote_summary",
    "final_label",
    "confidence",
    "evidence_files",
    "evidence_symbols",
    "rationale",
    "reviewer",
    "reviewed_at",
    "notes",
]


@dataclass(frozen=True)
class Question:
    task_id: str
    question_id: str
    question_type: str
    prompt: str
    correct_option: str
    correct_text: str
    options: dict[str, str]
    option_by_text: dict[str, str]
    source_requirement_id: str
    source_intention_id: str


@dataclass(frozen=True)
class Vote:
    vote_text: str
    canonical_option: str
    observations: int
    top_count: int
    agreement: float
    stable: bool
    selected_options: str


def main() -> None:
    args = _parse_args()
    p = paths()
    output_dir = args.output_dir or (p.reports_dir / "artifact_conformance_consensus")
    output_dir.mkdir(parents=True, exist_ok=True)
    include_compile_failed = args.include_compile_failed or not args.compile_pass_only
    answer_panel_id = args.panel_id or (
        "paper_final_scratch" if args.compile_pass_only else "artifact_present_diagnostic"
    )

    build_rows = _read_csv(p.tables_dir / "build_runs.csv")
    find_rows = _read_csv(p.tables_dir / "find_runs.csv")
    answer_rows = _read_csv(p.tables_dir / "question_answers.csv")
    questions = _load_questions(p.questions_dir)
    finder_metadata = _finder_metadata(find_rows)

    selected_base_models = tuple(args.base_model or DEFAULT_BASE_MODELS)
    selected_finder_efforts = tuple(args.finder_effort or DEFAULT_FINDER_EFFORTS)
    selected_answer_rows = _select_answer_rows(
        answer_rows,
        selected_base_models=selected_base_models,
        selected_finder_efforts=selected_finder_efforts,
        finder_metadata=finder_metadata,
        compile_pass_by_build_id={
            row["build_run_id"]: _bool(row["compile_pass"]) for row in build_rows
        },
        panel_id=answer_panel_id,
        include_compile_failed=include_compile_failed,
    )
    artifact_questions = _artifact_question_index(
        build_rows,
        questions,
        panel_id=args.artifact_panel_id,
        include_compile_failed=include_compile_failed,
    )

    finder_vote_rows, model_vote_rows, consensus_rows = _build_consensus_rows(
        artifact_questions=artifact_questions,
        selected_answer_rows=selected_answer_rows,
        questions=questions,
        selected_base_models=selected_base_models,
        min_model_agreement=args.min_model_agreement,
    )

    manual_rows = [
        row
        for row in consensus_rows
        if row["consensus_status"] != "consensus_prelabel"
    ]

    summary_rows = _summary_rows(consensus_rows)
    _write_csv(output_dir / "artifact_question_consensus.csv", consensus_rows, CONSENSUS_FIELD_ORDER)
    _write_csv(output_dir / "manual_review_queue.csv", manual_rows, CONSENSUS_FIELD_ORDER)
    _write_csv(output_dir / "model_votes.csv", model_vote_rows, VOTE_FIELD_ORDER)
    _write_csv(output_dir / "finder_votes.csv", finder_vote_rows, VOTE_FIELD_ORDER)
    _write_csv(output_dir / "summary.csv", summary_rows, SUMMARY_FIELD_ORDER)
    _write_manual_review_markdown(output_dir / "manual_review_queue.md", manual_rows)
    manual_annotation_rows = _manual_annotation_rows(manual_rows)
    _write_csv(
        output_dir / "manual_review_annotations.template.csv",
        manual_annotation_rows,
        MANUAL_ANNOTATION_FIELD_ORDER,
    )
    _write_csv_if_missing(
        output_dir / "manual_review_annotations.csv",
        manual_annotation_rows,
        MANUAL_ANNOTATION_FIELD_ORDER,
    )

    manifest = {
        "analysis_id": "artifact_conformance_consensus_v1",
        "analysis_script": "scripts/analyze_artifact_conformance_consensus.py",
        "analysis_command": " ".join(sys.argv),
        "generated_at": datetime.now(UTC).isoformat(),
        "output_dir": _display_path(output_dir),
        "input_paths": [
            _display_path(p.tables_dir / "build_runs.csv"),
            _display_path(p.tables_dir / "find_runs.csv"),
            _display_path(p.tables_dir / "question_answers.csv"),
            _display_path(p.questions_dir / "scratch_minidb.yaml"),
            _display_path(p.questions_dir / "scratch_nanoweb.yaml"),
        ],
        "input_hashes": {
            _display_path(path): _sha256(path)
            for path in (
                p.tables_dir / "build_runs.csv",
                p.tables_dir / "find_runs.csv",
                p.tables_dir / "question_answers.csv",
                p.questions_dir / "scratch_minidb.yaml",
                p.questions_dir / "scratch_nanoweb.yaml",
            )
            if path.exists()
        },
        "scope": {
            "artifact_panel_id": args.artifact_panel_id,
            "answer_panel_id": answer_panel_id,
            "include_compile_failed": include_compile_failed,
            "compile_pass_only": args.compile_pass_only,
            "base_models": list(selected_base_models),
            "finder_efforts": list(selected_finder_efforts),
            "consensus_unit": "base_model",
            "min_model_agreement": args.min_model_agreement,
        },
        "classification_rule": {
            "implemented_gold": (
                "all selected base-model votes are stable and agree with the canonical "
                "gold answer text"
            ),
            "implemented_non_gold": (
                "all selected base-model votes are stable and agree on the same "
                "non-gold answer text"
            ),
            "manual_review_required": (
                "missing answers, unstable within-model vote, or disagreement between "
                "selected base-model votes"
            ),
            "important_note": (
                "This is a triage prelabel derived from finder answer consensus. It is "
                "not an independent implementation-conformance annotation."
            ),
        },
        "final_conformance_labels": list(FINAL_CONFORMANCE_LABELS),
        "row_counts": {
            "artifact_question_consensus": len(consensus_rows),
            "manual_review_queue": len(manual_rows),
            "model_votes": len(model_vote_rows),
            "finder_votes": len(finder_vote_rows),
        },
    }
    (output_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    _write_markdown(output_dir / "README.md", manifest=manifest, summary_rows=summary_rows)
    print(f"wrote {output_dir}")


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build artifact-question conformance prelabels from finder consensus."
    )
    parser.add_argument(
        "--base-model",
        action="append",
        help=(
            "Finder base_model to include. Repeatable. Default includes GPT-5.5 "
            "and Opus 4.7."
        ),
    )
    parser.add_argument(
        "--finder-effort",
        action="append",
        help=(
            "Finder effort level to include. Repeatable. Defaults to high only."
        ),
    )
    parser.add_argument(
        "--panel-id",
        help=(
            "Panel id required on question-answer rows. Defaults to artifact_present_diagnostic, "
            "or paper_final_scratch when --compile-pass-only is set."
        ),
    )
    parser.add_argument(
        "--artifact-panel-id",
        default="artifact_present_diagnostic",
        help="Panel id required on build artifact rows.",
    )
    parser.add_argument(
        "--include-compile-failed",
        action="store_true",
        help="Include compile-probe-failed artifact-present builds. This is now the default.",
    )
    parser.add_argument(
        "--compile-pass-only",
        action="store_true",
        help="Restrict output to compile-pass primary artifacts.",
    )
    parser.add_argument(
        "--min-model-agreement",
        type=float,
        default=2 / 3,
        help="Minimum within-base-model top-answer share required for a stable vote.",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        help="Output directory. Defaults to reports/artifact_conformance_consensus.",
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
        writer = csv.DictWriter(handle, fieldnames=fieldnames, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


def _write_csv_if_missing(path: Path, rows: list[dict[str, Any]], preferred: list[str]) -> None:
    if path.exists():
        return
    _write_csv(path, rows, preferred)


def _load_questions(questions_dir: Path) -> dict[tuple[str, str], Question]:
    questions: dict[tuple[str, str], Question] = {}
    for path in sorted(questions_dir.glob("*.yaml")):
        data = yaml.safe_load(path.read_text(encoding="utf-8"))
        task_id = data["task_id"]
        for raw_question in data["questions"]:
            options = raw_question["options"]
            correct_option = raw_question["correct_option"]
            correct_text = options[correct_option]
            source_trace = raw_question["source_trace"]
            question = Question(
                task_id=task_id,
                question_id=raw_question["question_id"],
                question_type=raw_question["question_type"],
                prompt=_normalize_space(raw_question["prompt"]),
                correct_option=correct_option,
                correct_text=correct_text,
                options=options,
                option_by_text={value: key for key, value in options.items()},
                source_requirement_id=source_trace["source_requirement_id"],
                source_intention_id=source_trace["source_intention_id"],
            )
            questions[(task_id, question.question_id)] = question
    return questions


def _finder_metadata(find_rows: list[dict[str, str]]) -> dict[str, dict[str, str]]:
    metadata = {}
    for row in find_rows:
        finder_id = row["finder_id"]
        metadata[finder_id] = {
            "base_model": row["finder_model_id"].replace("-", "_").replace(".", "_"),
            "finder_family": row["finder_family"],
            "finder_effort": row["finder_effort"],
        }
    overrides = {
        "claude_code_opus_4_7_high": "claude_code_opus_4_7",
        "claude_code_opus_4_7_low": "claude_code_opus_4_7",
        "codex_gpt5_5_high": "codex_gpt5_5",
        "codex_gpt5_5_low": "codex_gpt5_5",
        "codex_gpt5_4_mini_high": "codex_gpt5_4_mini",
        "codex_gpt5_4_mini_low": "codex_gpt5_4_mini",
        "claude_code_sonnet_4_6_high": "claude_code_sonnet_4_6",
        "claude_code_sonnet_4_6_low": "claude_code_sonnet_4_6",
        "claude_code_mimo_v2_5_high": "claude_code_mimo_v2_5",
        "claude_code_mimo_v2_5_low": "claude_code_mimo_v2_5",
        "claude_code_mimo_v2_5_pro_high": "claude_code_mimo_v2_5_pro",
        "claude_code_mimo_v2_5_pro_low": "claude_code_mimo_v2_5_pro",
    }
    for finder_id, base_model in overrides.items():
        if finder_id in metadata:
            metadata[finder_id]["base_model"] = base_model
    return metadata


def _select_answer_rows(
    rows: list[dict[str, str]],
    *,
    selected_base_models: tuple[str, ...],
    selected_finder_efforts: tuple[str, ...],
    finder_metadata: dict[str, dict[str, str]],
    compile_pass_by_build_id: dict[str, bool | None],
    panel_id: str,
    include_compile_failed: bool,
) -> list[dict[str, str]]:
    selected = []
    selected_base_model_set = set(selected_base_models)
    selected_finder_effort_set = set(selected_finder_efforts)
    for row in rows:
        metadata = finder_metadata.get(row["finder_id"])
        if not metadata:
            continue
        if metadata["base_model"] not in selected_base_model_set:
            continue
        if metadata["finder_effort"] not in selected_finder_effort_set:
            continue
        if panel_id not in _panel_ids(row):
            continue
        if not include_compile_failed and compile_pass_by_build_id.get(row["build_run_id"]) is not True:
            continue
        if row["build_run_id"].startswith("control-build-"):
            continue
        if row["answered"] != "true":
            continue
        selected.append({**row, **metadata})
    return selected


def _artifact_question_index(
    build_rows: list[dict[str, str]],
    questions: dict[tuple[str, str], Question],
    *,
    panel_id: str,
    include_compile_failed: bool,
) -> list[dict[str, str]]:
    artifacts = []
    for row in build_rows:
        if row["builder_id"].startswith("control_"):
            continue
        if panel_id not in _panel_ids(row):
            continue
        compile_pass = _bool(row["compile_pass"])
        if not include_compile_failed and compile_pass is not True:
            continue
        artifacts.append(row)

    out = []
    for artifact in artifacts:
        task_questions = sorted(
            (
                question
                for (task_id, _), question in questions.items()
                if task_id == artifact["task_id"]
            ),
            key=lambda question: question.question_id,
        )
        for question in task_questions:
            out.append(
                {
                    "artifact_id": artifact["build_run_id"],
                    "artifact_path": artifact["artifact_path"],
                    "task_id": artifact["task_id"],
                    "builder_id": artifact["builder_id"],
                    "builder_family": artifact["builder_family"],
                    "builder_effort": artifact["builder_effort"],
                    "build_trial_id": artifact["build_trial_id"],
                    "compile_pass": artifact["compile_pass"],
                    "artifact_exists": artifact["artifact_exists"],
                    "rust_files": artifact["rust_files"],
                    "rust_loc": artifact["rust_loc"],
                    "question_id": question.question_id,
                }
            )
    return sorted(out, key=lambda row: (row["task_id"], row["artifact_id"], row["question_id"]))


def _build_consensus_rows(
    *,
    artifact_questions: list[dict[str, str]],
    selected_answer_rows: list[dict[str, str]],
    questions: dict[tuple[str, str], Question],
    selected_base_models: tuple[str, ...],
    min_model_agreement: float,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]]]:
    rows_by_key: dict[tuple[str, str, str], list[dict[str, str]]] = defaultdict(list)
    for row in selected_answer_rows:
        rows_by_key[(row["build_run_id"], row["task_id"], row["question_id"])].append(row)

    finder_vote_rows = []
    model_vote_rows = []
    consensus_rows = []

    for artifact_question in artifact_questions:
        artifact_id = artifact_question["artifact_id"]
        task_id = artifact_question["task_id"]
        question_id = artifact_question["question_id"]
        question = questions[(task_id, question_id)]
        answer_rows = rows_by_key[(artifact_id, task_id, question_id)]

        finder_votes = _finder_votes(answer_rows, question, min_model_agreement)
        for finder_id, vote in sorted(finder_votes.items()):
            finder_vote_rows.append(
                {
                    **_base_output_fields(artifact_question, question),
                    "finder_id": finder_id,
                    "finder_base_model": _base_model_for_finder(answer_rows, finder_id),
                    "vote_text": vote.vote_text,
                    "vote_canonical_option": vote.canonical_option,
                    "observations": vote.observations,
                    "top_count": vote.top_count,
                    "agreement": _fmt_float(vote.agreement),
                    "stable": str(vote.stable).lower(),
                    "selected_options": vote.selected_options,
                }
            )

        model_votes = _model_votes(answer_rows, question, selected_base_models, min_model_agreement)
        for base_model, vote in sorted(model_votes.items()):
            model_vote_rows.append(
                {
                    **_base_output_fields(artifact_question, question),
                    "base_model": base_model,
                    "vote_text": vote.vote_text,
                    "vote_canonical_option": vote.canonical_option,
                    "observations": vote.observations,
                    "top_count": vote.top_count,
                    "agreement": _fmt_float(vote.agreement),
                    "stable": str(vote.stable).lower(),
                    "selected_options": vote.selected_options,
                }
            )

        consensus = _consensus(model_votes, question, selected_base_models)
        consensus_rows.append(
            {
                **_base_output_fields(artifact_question, question),
                **consensus,
                "selected_base_models": "|".join(selected_base_models),
                "selected_answer_rows": len(answer_rows),
                "allowed_final_labels": "|".join(FINAL_CONFORMANCE_LABELS),
            }
        )

    return finder_vote_rows, model_vote_rows, consensus_rows


def _finder_votes(
    rows: list[dict[str, str]],
    question: Question,
    min_agreement: float,
) -> dict[str, Vote]:
    grouped: dict[str, list[dict[str, str]]] = defaultdict(list)
    for row in rows:
        grouped[row["finder_id"]].append(row)
    return {
        finder_id: _vote(group, question, min_agreement)
        for finder_id, group in grouped.items()
    }


def _model_votes(
    rows: list[dict[str, str]],
    question: Question,
    selected_base_models: tuple[str, ...],
    min_agreement: float,
) -> dict[str, Vote]:
    grouped: dict[str, list[dict[str, str]]] = {base_model: [] for base_model in selected_base_models}
    for row in rows:
        if row["base_model"] in grouped:
            grouped[row["base_model"]].append(row)
    return {
        base_model: _vote(group, question, min_agreement)
        for base_model, group in grouped.items()
    }


def _vote(rows: list[dict[str, str]], question: Question, min_agreement: float) -> Vote:
    if not rows:
        return Vote(
            vote_text="",
            canonical_option="",
            observations=0,
            top_count=0,
            agreement=math.nan,
            stable=False,
            selected_options="",
        )
    counts = Counter(row["selected_text"] for row in rows)
    ranked = counts.most_common()
    vote_text, top_count = ranked[0]
    tie = len(ranked) > 1 and ranked[1][1] == top_count
    agreement = top_count / len(rows)
    stable = (not tie) and agreement >= min_agreement
    return Vote(
        vote_text=vote_text if stable else "",
        canonical_option=question.option_by_text.get(vote_text, "") if stable else "",
        observations=len(rows),
        top_count=top_count,
        agreement=agreement,
        stable=stable,
        selected_options=_option_counts(rows, question),
    )


def _consensus(
    model_votes: dict[str, Vote],
    question: Question,
    selected_base_models: tuple[str, ...],
) -> dict[str, str]:
    missing = [
        base_model
        for base_model in selected_base_models
        if model_votes[base_model].observations == 0
    ]
    unstable = [
        base_model
        for base_model in selected_base_models
        if model_votes[base_model].observations > 0 and not model_votes[base_model].stable
    ]
    stable_votes = [
        model_votes[base_model].vote_text
        for base_model in selected_base_models
        if model_votes[base_model].stable
    ]
    if missing:
        return _manual_review(
            reason=f"missing selected base-model answers: {', '.join(missing)}",
            model_votes=model_votes,
            selected_base_models=selected_base_models,
        )
    if unstable:
        return _manual_review(
            reason=f"unstable within-base-model votes: {', '.join(unstable)}",
            model_votes=model_votes,
            selected_base_models=selected_base_models,
        )
    if len(set(stable_votes)) != 1:
        return _manual_review(
            reason="selected base models disagree",
            model_votes=model_votes,
            selected_base_models=selected_base_models,
        )

    consensus_text = stable_votes[0]
    consensus_option = question.option_by_text.get(consensus_text, "")
    label = (
        "implemented_gold"
        if consensus_text == question.correct_text
        else "implemented_non_gold"
    )
    return {
        "consensus_status": "consensus_prelabel",
        "preliminary_label": label,
        "reviewer_action": "spot_check_or_accept_policy",
        "consensus_text": consensus_text,
        "consensus_canonical_option": consensus_option,
        "consensus_matches_gold": str(consensus_text == question.correct_text).lower(),
        "consensus_reason": "all selected base-model votes are stable and agree",
        "model_vote_summary": _model_vote_summary(model_votes, selected_base_models),
    }


def _manual_review(
    *,
    reason: str,
    model_votes: dict[str, Vote],
    selected_base_models: tuple[str, ...],
) -> dict[str, str]:
    return {
        "consensus_status": "manual_review_required",
        "preliminary_label": "",
        "reviewer_action": "manual_classify_with_final_five_labels",
        "consensus_text": "",
        "consensus_canonical_option": "",
        "consensus_matches_gold": "",
        "consensus_reason": reason,
        "model_vote_summary": _model_vote_summary(model_votes, selected_base_models),
    }


def _base_output_fields(artifact_question: dict[str, str], question: Question) -> dict[str, str]:
    return {
        **artifact_question,
        "question_type": question.question_type,
        "question_prompt": question.prompt,
        "gold_claim_polarity": (
            "negative_contrast_inconsistent_option"
            if question.question_type == "negative_contrast"
            else "positive_or_direct_claim"
        ),
        "source_requirement_id": question.source_requirement_id,
        "source_intention_id": question.source_intention_id,
        "gold_canonical_option": question.correct_option,
        "gold_text": question.correct_text,
        "option_a": question.options["A"],
        "option_b": question.options["B"],
        "option_c": question.options["C"],
        "option_d": question.options["D"],
    }


def _summary_rows(consensus_rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    groups: dict[tuple[str, str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in consensus_rows:
        groups[("all", "all", "all")].append(row)
        groups[(row["task_id"], "all", "all")].append(row)
        groups[(row["task_id"], row["compile_pass"], "all")].append(row)
        groups[(row["task_id"], row["compile_pass"], row["builder_id"])].append(row)

    out = []
    for (task_id, compile_pass, builder_id), rows in sorted(groups.items()):
        counts = Counter(
            row["preliminary_label"] or row["consensus_status"]
            for row in rows
        )
        out.append(
            {
                "task_id": task_id,
                "compile_pass": compile_pass,
                "builder_id": builder_id,
                "artifact_question_pairs": len(rows),
                "implemented_gold": counts["implemented_gold"],
                "implemented_non_gold": counts["implemented_non_gold"],
                "manual_review_required": counts["manual_review_required"],
                "implemented_gold_rate": _fmt_float(counts["implemented_gold"] / len(rows)),
                "implemented_non_gold_rate": _fmt_float(
                    counts["implemented_non_gold"] / len(rows)
                ),
                "manual_review_rate": _fmt_float(
                    counts["manual_review_required"] / len(rows)
                ),
            }
        )
    return out


def _manual_annotation_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    out = []
    for row in rows:
        out.append(
            {
                "task_id": row["task_id"],
                "artifact_id": row["artifact_id"],
                "artifact_path": row["artifact_path"],
                "builder_id": row["builder_id"],
                "build_trial_id": row["build_trial_id"],
                "compile_pass": row["compile_pass"],
                "question_id": row["question_id"],
                "question_type": row["question_type"],
                "question_prompt": row["question_prompt"],
                "gold_claim_polarity": row["gold_claim_polarity"],
                "gold_canonical_option": row["gold_canonical_option"],
                "gold_text": row["gold_text"],
                "option_a": row["option_a"],
                "option_b": row["option_b"],
                "option_c": row["option_c"],
                "option_d": row["option_d"],
                "consensus_reason": row["consensus_reason"],
                "model_vote_summary": row["model_vote_summary"],
                "final_label": "",
                "confidence": "",
                "evidence_files": "",
                "evidence_symbols": "",
                "rationale": "",
                "reviewer": "",
                "reviewed_at": "",
                "notes": "",
            }
        )
    return out


def _write_markdown(
    path: Path,
    *,
    manifest: dict[str, Any],
    summary_rows: list[dict[str, Any]],
) -> None:
    overall = next(row for row in summary_rows if row["task_id"] == "all")
    lines = [
        "# Artifact Conformance Consensus Triage",
        "",
        "This directory is a derived pre-audit layer. It uses selected finder answer "
        "consensus to queue artifact-question pairs for conformance annotation; it is "
        "not an independent correctness oracle.",
        "",
        "## Scope",
        "",
        f"- Base models: `{', '.join(manifest['scope']['base_models'])}`",
        f"- Finder efforts: `{', '.join(manifest['scope']['finder_efforts'])}`",
        f"- Consensus unit: `{manifest['scope']['consensus_unit']}`",
        f"- Minimum within-model agreement: `{manifest['scope']['min_model_agreement']}`",
        f"- Include compile-failed artifacts: `{manifest['scope']['include_compile_failed']}`",
        "",
        "## Overall Counts",
        "",
        "| pairs | implemented_gold | implemented_non_gold | manual_review_required |",
        "| ---: | ---: | ---: | ---: |",
        (
            f"| {overall['artifact_question_pairs']} | {overall['implemented_gold']} | "
            f"{overall['implemented_non_gold']} | {overall['manual_review_required']} |"
        ),
        "",
        "## Files",
        "",
        "- `artifact_question_consensus.csv`: one row per artifact-question pair.",
        "- `manual_review_queue.csv`: rows without selected-model consensus.",
        "- `manual_review_queue.md`: human-readable list of rows to inspect.",
        "- `manual_review_annotations.csv`: fill this file with final manual labels.",
        "- `manual_review_annotations.template.csv`: regenerated blank annotation template.",
        "- `model_votes.csv`: one row per artifact-question-base-model vote.",
        "- `finder_votes.csv`: one row per artifact-question-finder-id vote.",
        "- `summary.csv`: aggregate counts by task, compile status, and builder.",
        "- `manifest.json`: inputs, hashes, command, and classification rule.",
        "",
        "Final manual labels should use:",
        "",
        ", ".join(f"`{label}`" for label in FINAL_CONFORMANCE_LABELS),
        "",
    ]
    path.write_text("\n".join(lines), encoding="utf-8")


def _write_manual_review_markdown(path: Path, rows: list[dict[str, Any]]) -> None:
    lines = [
        "# Manual Review Queue",
        "",
        "These artifact-question pairs did not receive a stable high-effort consensus prelabel.",
        "",
    ]
    if not rows:
        lines.extend(["No rows require manual review.", ""])
        path.write_text("\n".join(lines), encoding="utf-8")
        return

    lines.extend(
        [
            "| task | builder | trial | question | prompt/options | reason | votes | artifact |",
            "| --- | --- | ---: | --- | --- | --- | --- | --- |",
        ]
    )
    for row in rows:
        artifact = f"`{row['artifact_id']}`"
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{row['task_id']}`",
                    f"`{row['builder_id']}`",
                    row["build_trial_id"],
                    f"`{row['question_id']}`",
                    _question_review_cell(row),
                    _escape_md(_anonymize_voters(row["consensus_reason"], row)),
                    _escape_md(_anonymize_voters(row["model_vote_summary"], row)),
                    artifact,
                ]
            )
            + " |"
        )
    lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")


def _escape_md(value: Any) -> str:
    return str(value).replace("|", "\\|").replace("\n", " ")


def _question_review_cell(row: dict[str, Any]) -> str:
    parts = [
        f"Prompt: {_escape_md(row['question_prompt'])}",
        f"A: {_escape_md(row['option_a'])}",
        f"B: {_escape_md(row['option_b'])}",
        f"C: {_escape_md(row['option_c'])}",
        f"D: {_escape_md(row['option_d'])}",
        f"Gold: `{row['gold_canonical_option']}`",
    ]
    return "<br>".join(parts)


def _anonymize_voters(value: Any, row: dict[str, Any]) -> str:
    text = str(value)
    for index, voter in enumerate(str(row.get("selected_base_models", "")).split("|"), start=1):
        if voter:
            text = text.replace(voter, f"voter{index}")
    return text


def _base_model_for_finder(rows: list[dict[str, str]], finder_id: str) -> str:
    for row in rows:
        if row["finder_id"] == finder_id:
            return row["base_model"]
    return ""


def _option_counts(rows: list[dict[str, str]], question: Question) -> str:
    counts = Counter(
        question.option_by_text.get(row["selected_text"], "?")
        for row in rows
    )
    return "|".join(f"{option}:{count}" for option, count in sorted(counts.items()))


def _model_vote_summary(
    model_votes: dict[str, Vote],
    selected_base_models: tuple[str, ...],
) -> str:
    parts = []
    for base_model in selected_base_models:
        vote = model_votes[base_model]
        if vote.observations == 0:
            parts.append(f"{base_model}=missing")
        elif vote.stable:
            parts.append(
                f"{base_model}={vote.canonical_option} "
                f"({vote.top_count}/{vote.observations})"
            )
        else:
            parts.append(
                f"{base_model}=unstable {vote.selected_options} "
                f"({vote.top_count}/{vote.observations})"
            )
    return "; ".join(parts)


def _panel_ids(row: dict[str, str]) -> set[str]:
    return set(row.get("panel_ids", "").split("|"))


def _bool(value: str) -> bool | None:
    if value == "true":
        return True
    if value == "false":
        return False
    return None


def _fmt_float(value: float) -> str:
    if math.isnan(value):
        return ""
    return f"{value:.6f}"


def _normalize_space(value: Any) -> str:
    return " ".join(str(value).split())


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
