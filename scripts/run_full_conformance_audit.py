"""Run the full-coverage live-path artifact conformance audit.

The runner gives each artifact a fresh read-only Codex session, preserves every
session attempt as append-only raw evidence, validates file/line citations, performs
the required independent cross-check, and deterministically builds the requested
canonical CSV/report outputs without modifying ``runs_release``.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import os
import re
from collections import Counter, defaultdict
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path, PurePosixPath
from typing import Any, Iterable, Mapping, Sequence

import yaml


ROOT = Path(__file__).resolve().parents[1]
RUNS_RELEASE = ROOT / "runs_release"
DEFAULT_OUTPUT_DIR = ROOT / "rebuttal" / "full_conformance_audit"
LABELS_PATH = (
    RUNS_RELEASE
    / "reports"
    / "artifact_conformance_consensus"
    / "final"
    / "artifact_conformance_final.csv"
)
QUESTION_PATHS = {
    "scratch_minidb": ROOT / "benchmarks" / "questions" / "scratch_minidb.yaml",
    "scratch_nanoweb": ROOT / "benchmarks" / "questions" / "scratch_nanoweb.yaml",
}
TASK_ORDER = ("scratch_minidb", "scratch_nanoweb")
EXPECTED_TASK_PAIRS = {"scratch_minidb": 302, "scratch_nanoweb": 290}
EXPECTED_TASK_ARTIFACTS = {"scratch_minidb": 21, "scratch_nanoweb": 20}
EXPECTED_PAIRS = 592
EXPECTED_ARTIFACTS = 41

PROMPT_VERSION = "full_conformance_live_path_v1"
SPEC_VERSION = "v1.2"
INTEGRITY_ATTESTATION_VERSION = "full_conformance_integrity_attestation_v1"
GENERATION_MANIFEST_VERSION = "full_conformance_generation_v1"
SENSITIVITY_MANIFEST_VERSION = "rebuttal_label_sensitivity_v2"
SENSITIVITY_INPUT_KEYS = {
    "final_labels",
    "find_runs",
    "question_answers",
    "low_prior_manifest",
    "published_builder_scores",
    "spot_audit_minidb",
    "spot_audit_nanoweb",
    "spot_audit_sample_manifest",
    "full_audit_disagreements",
    "full_audit_minidb_adjudication",
    "full_audit_nanoweb_adjudication",
    "full_audit_validation_manifest",
    "full_audit_core_generation",
}
PROMPT_TEMPLATE_PATH = DEFAULT_OUTPUT_DIR / "prompts" / "live_path_v1.md"
RESULT_SCHEMA_PATH = DEFAULT_OUTPUT_DIR / "result_schema.json"
CROSSCHECK_SEED = 20260728
CROSSCHECK_RATE = 0.10

PRIMARY_CONFIG = {
    "model_id": "gpt-5.6-sol",
    "reasoning_effort": "ultra",
    "pass_id": "primary",
}
CROSSCHECK_CONFIG = {
    "model_id": "gpt-5.6-terra",
    "reasoning_effort": "ultra",
    "pass_id": "crosscheck",
}
VERDICTS = {"confirmed_gold", "non_gold", "absent", "ambiguous"}
ACTUAL_OPTIONS = {"A", "B", "C", "D", "none", "multiple", "unclear"}
CONFIDENCES = {"high", "medium", "low"}
KNOWN_REGRESSIONS = {
    (
        "scratch_minidb",
        ("build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3"),
        "q_db_05",
    ): "implemented_non_gold",
    (
        "scratch_nanoweb",
        ("build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601"),
        "q_nw_07",
    ): "absent_or_ambiguous",
    (
        "scratch_minidb",
        ("build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95"),
        "q_db_02",
    ): "absent_or_ambiguous",
    (
        "scratch_nanoweb",
        ("build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51"),
        "q_nw_13",
    ): "absent_or_ambiguous",
    (
        "scratch_nanoweb",
        ("build-scratch_nanoweb-claude_code_opus_4_7_high-t1-s20261502-374bc791"),
        "q_nw_14",
    ): "absent_or_ambiguous",
    (
        "scratch_nanoweb",
        ("build-scratch_nanoweb-claude_code_opus_4_7_low-t1-s20261502-ae88af88"),
        "q_nw_11",
    ): "absent_or_ambiguous",
    (
        "scratch_nanoweb",
        ("build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95"),
        "q_nw_03",
    ): "absent_or_ambiguous",
    (
        "scratch_nanoweb",
        ("build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480"),
        "q_nw_02",
    ): "absent_or_ambiguous",
    (
        "scratch_nanoweb",
        ("build-scratch_nanoweb-claude_code_mimo_v2_5_low-t1-s20260502-0ab2ca9f"),
        "q_nw_02",
    ): "absent_or_ambiguous",
}

PRIMARY_FIELDS = (
    "sample_index",
    "task_id",
    "artifact_id",
    "question_id",
    "consensus_label",
    "source_audit_label",
    "agrees_with_consensus",
    "verdict",
    "actual_option",
    "confidence",
    "runtime_reachability_checked",
    "evidence_files",
    "evidence_symbols",
    "evidence_spans",
    "rationale",
    "adjudicator",
    "execution_engine",
    "model_id",
    "reasoning_effort",
    "prompt_version",
    "prompt_template_sha256",
    "rendered_prompt_sha256",
    "codex_cli_version",
    "session_id",
    "session_id_source",
    "model_id_source",
    "session_result_sha256",
    "session_result_hash_source",
    "integrity_attestation_sha256",
    "artifact_path",
    "builder_id",
    "builder_family",
    "builder_effort",
    "build_trial_id",
    "question_type",
    "gold_canonical_option",
    "source_requirement_id",
    "source_intention_id",
    "current_label_source",
    "crosscheck_selected",
    "crosscheck_selection_reason",
    "crosscheck_verdict",
    "crosscheck_source_audit_label",
    "crosscheck_actual_option",
    "crosscheck_confidence",
    "crosscheck_runtime_reachability_checked",
    "crosscheck_evidence_files",
    "crosscheck_evidence_symbols",
    "crosscheck_evidence_spans",
    "crosscheck_rationale",
    "crosscheck_execution_engine",
    "crosscheck_model_id",
    "crosscheck_reasoning_effort",
    "crosscheck_prompt_version",
    "crosscheck_prompt_template_sha256",
    "crosscheck_rendered_prompt_sha256",
    "crosscheck_codex_cli_version",
    "crosscheck_session_id",
    "crosscheck_session_id_source",
    "crosscheck_model_id_source",
    "crosscheck_session_result_sha256",
    "crosscheck_session_result_hash_source",
    "crosscheck_integrity_attestation_sha256",
    "crosscheck_agrees_with_primary",
    "review_status",
    "flagged_for_author",
)

DISAGREEMENT_FIELDS = (
    "task_id",
    "artifact_id",
    "question_id",
    "builder_id",
    "builder_family",
    "builder_effort",
    "consensus_label",
    "current_label_source",
    "primary_verdict",
    "primary_source_audit_label",
    "primary_confidence",
    "primary_evidence_files",
    "primary_evidence_symbols",
    "primary_evidence_spans",
    "primary_rationale",
    "primary_execution_engine",
    "primary_model_id",
    "primary_reasoning_effort",
    "primary_prompt_version",
    "primary_prompt_template_sha256",
    "primary_rendered_prompt_sha256",
    "primary_session_id",
    "primary_session_id_source",
    "primary_model_id_source",
    "primary_session_result_sha256",
    "primary_session_result_hash_source",
    "primary_integrity_attestation_sha256",
    "crosscheck_verdict",
    "crosscheck_source_audit_label",
    "crosscheck_confidence",
    "crosscheck_evidence_files",
    "crosscheck_evidence_symbols",
    "crosscheck_evidence_spans",
    "crosscheck_rationale",
    "crosscheck_execution_engine",
    "crosscheck_model_id",
    "crosscheck_reasoning_effort",
    "crosscheck_prompt_version",
    "crosscheck_prompt_template_sha256",
    "crosscheck_rendered_prompt_sha256",
    "crosscheck_session_id",
    "crosscheck_session_id_source",
    "crosscheck_model_id_source",
    "crosscheck_session_result_sha256",
    "crosscheck_session_result_hash_source",
    "crosscheck_integrity_attestation_sha256",
    "crosscheck_agrees_with_primary",
    "disagreement_basis",
    "flag_verdicts",
    "proposed_sensitivity_label",
    "author_adjudication_status",
)

REGRESSION_FIELDS = (
    "task_id",
    "artifact_id",
    "question_id",
    "expected_mapped_label",
    "primary_verdict",
    "primary_mapped_label",
    "crosscheck_verdict",
    "crosscheck_mapped_label",
    "primary_flag",
    "crosscheck_flag",
    "primary_reproduced_expected_label",
    "crosscheck_reproduced_expected_label",
    "reproduced_by_either_pass",
    "reproduced_by_both_passes",
)


@dataclass(frozen=True)
class Pair:
    source: dict[str, str]
    question: dict[str, Any]

    @property
    def key(self) -> tuple[str, str, str]:
        return (
            self.source["task_id"],
            self.source["artifact_id"],
            self.source["question_id"],
        )

    @property
    def artifact_key(self) -> tuple[str, str]:
        return (self.source["task_id"], self.source["artifact_id"])


@dataclass(frozen=True)
class SessionResult:
    pass_id: str
    artifact_id: str
    task_id: str
    attempt_dir: Path
    payload: dict[str, Any]
    provenance: dict[str, Any]

    @property
    def rows_by_question(self) -> dict[str, dict[str, Any]]:
        return {row["question_id"]: row for row in self.payload["rows"]}


def main() -> None:
    args = parse_args()
    output_dir = args.output_dir.resolve()
    assert_additive_output(output_dir)
    prompt_template_path = (
        args.prompt_template.resolve()
        if args.prompt_template
        else output_dir / "prompts" / "live_path_v1.md"
    )
    result_schema_path = (
        args.result_schema.resolve() if args.result_schema else output_dir / "result_schema.json"
    )
    require_files(
        [
            LABELS_PATH,
            *QUESTION_PATHS.values(),
            prompt_template_path,
            result_schema_path,
            output_dir / "SPEC.md",
        ]
    )

    pairs = load_population()
    grouped = group_population(pairs)
    phase = args.phase
    if phase == "prepare":
        prepare(
            output_dir=output_dir,
            pairs=pairs,
            grouped=grouped,
            prompt_template_path=prompt_template_path,
            result_schema_path=result_schema_path,
        )
        print(f"prepared audit manifest for {len(pairs)} pairs", flush=True)
        return
    require_files([output_dir / "execution_manifest.json"])

    selected_artifacts = ordered_artifact_keys(grouped)
    if args.artifact_limit is not None:
        selected_artifacts = selected_artifacts[: args.artifact_limit]

    if phase == "agent-primary-prepare":
        prepare_agent_jobs(
            pass_config=PRIMARY_CONFIG,
            output_dir=output_dir,
            grouped=grouped,
            artifact_keys=selected_artifacts,
            selected_questions=None,
            prompt_template_path=prompt_template_path,
            result_schema_path=result_schema_path,
        )
        return

    if phase == "agent-primary-validate":
        validate_agent_jobs(
            pass_config=PRIMARY_CONFIG,
            output_dir=output_dir,
            grouped=grouped,
            selected_questions=None,
        )
        return

    if phase in {
        "select-crosscheck",
        "agent-crosscheck-prepare",
    }:
        require_complete_primary(output_dir, grouped)
        selection = freeze_crosscheck_selection(output_dir, pairs, grouped)
    elif phase == "finalize":
        selection = load_crosscheck_selection(output_dir)
    else:
        selection = []

    if phase == "agent-crosscheck-prepare":
        crosscheck_artifacts, selected_questions = crosscheck_work(selection)
        prepare_agent_jobs(
            pass_config=CROSSCHECK_CONFIG,
            output_dir=output_dir,
            grouped=grouped,
            artifact_keys=crosscheck_artifacts,
            selected_questions=selected_questions,
            prompt_template_path=prompt_template_path,
            result_schema_path=result_schema_path,
        )
        return

    if phase == "agent-crosscheck-validate":
        selection = load_crosscheck_selection(output_dir)
        _, selected_questions = crosscheck_work(selection)
        validate_agent_jobs(
            pass_config=CROSSCHECK_CONFIG,
            output_dir=output_dir,
            grouped=grouped,
            selected_questions=selected_questions,
        )
        return

    if phase == "attest-integrity":
        attest_validated_results(output_dir=output_dir, grouped=grouped)
        return

    if phase == "finalize":
        require_complete_primary(output_dir, grouped)
        selection = load_crosscheck_selection(output_dir)
        require_complete_crosscheck(output_dir, selection)
        finalize(output_dir, pairs, grouped, selection)
        verify_frozen_inputs(output_dir, grouped)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--phase",
        choices=(
            "prepare",
            "agent-primary-prepare",
            "agent-primary-validate",
            "select-crosscheck",
            "agent-crosscheck-prepare",
            "agent-crosscheck-validate",
            "attest-integrity",
            "finalize",
        ),
        default="prepare",
    )
    parser.add_argument(
        "--engine",
        choices=("collaboration",),
        default="collaboration",
    )
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR)
    parser.add_argument("--prompt-template", type=Path)
    parser.add_argument("--result-schema", type=Path)
    parser.add_argument(
        "--artifact-limit",
        type=int,
        help="Prepare only the first N primary collaboration-agent assignments.",
    )
    args = parser.parse_args()
    if args.artifact_limit is not None and args.artifact_limit < 1:
        parser.error("--artifact-limit must be positive")
    if args.artifact_limit is not None and args.phase != "agent-primary-prepare":
        parser.error("--artifact-limit is only valid for agent-primary-prepare")
    return args


def assert_additive_output(output_dir: Path) -> None:
    runs_root = RUNS_RELEASE.resolve()
    if output_dir == runs_root or runs_root in output_dir.parents:
        raise SystemExit(f"refusing to write inside runs_release: {output_dir}")


def require_files(paths: Iterable[Path]) -> None:
    missing = [path for path in paths if not path.is_file()]
    if missing:
        raise SystemExit("missing required files: " + ", ".join(map(str, missing)))


def load_population() -> list[Pair]:
    question_banks = load_question_banks()
    with LABELS_PATH.open(newline="", encoding="utf-8") as handle:
        label_rows = list(csv.DictReader(handle))
    pairs: list[Pair] = []
    for row in label_rows:
        if row["compile_pass"].strip().lower() != "true":
            continue
        if row["final_label"] != "implemented_gold":
            continue
        task_id = row["task_id"]
        question_id = row["question_id"]
        question = question_banks[task_id].get(question_id)
        if question is None:
            raise RuntimeError(f"missing canonical question {task_id}:{question_id}")
        validate_question_metadata(row, question)
        artifact_path = ROOT / row["artifact_path"]
        if not artifact_path.is_dir():
            raise RuntimeError(f"missing artifact path: {artifact_path}")
        if not (artifact_path / "Cargo.toml").is_file():
            raise RuntimeError(f"compile-pass artifact lacks Cargo.toml: {artifact_path}")
        pairs.append(Pair(source=row, question=question))

    pairs.sort(key=lambda pair: pair_sort_key(pair.key))
    if len(pairs) != EXPECTED_PAIRS:
        raise RuntimeError(f"expected {EXPECTED_PAIRS} pairs, found {len(pairs)}")
    task_counts = Counter(pair.source["task_id"] for pair in pairs)
    if task_counts != Counter(EXPECTED_TASK_PAIRS):
        raise RuntimeError(f"unexpected task pair counts: {task_counts}")
    keys = [pair.key for pair in pairs]
    if len(keys) != len(set(keys)):
        raise RuntimeError("duplicate artifact-question key in audit population")
    artifact_counts = Counter()
    for task_id in TASK_ORDER:
        artifact_counts[task_id] = len(
            {pair.source["artifact_id"] for pair in pairs if pair.source["task_id"] == task_id}
        )
    if artifact_counts != Counter(EXPECTED_TASK_ARTIFACTS):
        raise RuntimeError(f"unexpected task artifact counts: {artifact_counts}")
    if len({pair.artifact_key for pair in pairs}) != EXPECTED_ARTIFACTS:
        raise RuntimeError("unexpected total artifact count")
    missing_regressions = sorted(set(KNOWN_REGRESSIONS) - set(keys))
    if missing_regressions:
        raise RuntimeError(f"known regression keys missing from population: {missing_regressions}")
    return pairs


def load_question_banks() -> dict[str, dict[str, dict[str, Any]]]:
    banks: dict[str, dict[str, dict[str, Any]]] = {}
    for task_id, path in QUESTION_PATHS.items():
        loaded = yaml.safe_load(path.read_text(encoding="utf-8"))
        if loaded.get("task_id") != task_id:
            raise RuntimeError(f"question bank task mismatch: {path}")
        questions = loaded.get("questions")
        if not isinstance(questions, list) or len(questions) != 15:
            raise RuntimeError(f"expected 15 questions in {path}")
        by_id = {question["question_id"]: question for question in questions}
        if len(by_id) != len(questions):
            raise RuntimeError(f"duplicate question ids in {path}")
        banks[task_id] = by_id
    return banks


def validate_question_metadata(row: Mapping[str, str], question: Mapping[str, Any]) -> None:
    trace = question["source_trace"]
    checks = {
        "question_type": question["question_type"],
        "question_prompt": normalize_space(question["prompt"]),
        "gold_canonical_option": question["correct_option"],
        "source_requirement_id": trace["source_requirement_id"],
        "source_intention_id": trace["source_intention_id"],
    }
    for field, expected in checks.items():
        actual = normalize_space(row[field]) if field == "question_prompt" else row[field]
        if actual != expected:
            raise RuntimeError(
                f"question metadata drift for {row['task_id']}:{row['question_id']} "
                f"{field}: {actual!r} != {expected!r}"
            )
    for option in ("A", "B", "C", "D"):
        if row[f"option_{option.lower()}"] != question["options"][option]:
            raise RuntimeError(f"option drift for {row['task_id']}:{row['question_id']}:{option}")


def normalize_space(value: str) -> str:
    return " ".join(value.split())


def group_population(
    pairs: Sequence[Pair],
) -> dict[tuple[str, str], list[Pair]]:
    grouped: dict[tuple[str, str], list[Pair]] = defaultdict(list)
    for pair in pairs:
        grouped[pair.artifact_key].append(pair)
    for artifact_pairs in grouped.values():
        artifact_pairs.sort(key=lambda pair: pair.source["question_id"])
    return dict(grouped)


def pair_sort_key(key: tuple[str, str, str]) -> tuple[int, str, str]:
    return (TASK_ORDER.index(key[0]), key[1], key[2])


def artifact_sort_key(key: tuple[str, str]) -> tuple[int, str]:
    return (TASK_ORDER.index(key[0]), key[1])


def ordered_artifact_keys(
    grouped: Mapping[tuple[str, str], Sequence[Pair]],
) -> list[tuple[str, str]]:
    return sorted(grouped, key=artifact_sort_key)


def prepare(
    *,
    output_dir: Path,
    pairs: Sequence[Pair],
    grouped: Mapping[tuple[str, str], Sequence[Pair]],
    prompt_template_path: Path,
    result_schema_path: Path,
) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    write_population_manifest(output_dir / "population_manifest.csv", pairs)
    artifact_hashes = compute_artifact_hashes(grouped)
    write_csv(
        output_dir / "artifact_manifest.csv",
        [
            {
                "processing_index": index,
                "task_id": task_id,
                "artifact_id": artifact_id,
                "artifact_path": grouped[(task_id, artifact_id)][0].source["artifact_path"],
                "pair_count": len(grouped[(task_id, artifact_id)]),
                "tree_sha256": artifact_hashes[(task_id, artifact_id)],
            }
            for index, (task_id, artifact_id) in enumerate(ordered_artifact_keys(grouped), start=1)
        ],
        (
            "processing_index",
            "task_id",
            "artifact_id",
            "artifact_path",
            "pair_count",
            "tree_sha256",
        ),
    )
    execution_manifest = {
        "experiment_id": "full_conformance_audit_v1",
        "spec_path": display_path(output_dir / "SPEC.md"),
        "population_definition": (
            "compile_pass=true AND final_label=implemented_gold in frozen final labels"
        ),
        "population_pairs": len(pairs),
        "population_artifacts": len(grouped),
        "task_order": list(TASK_ORDER),
        "pair_counts": dict(Counter(pair.source["task_id"] for pair in pairs)),
        "artifact_counts": {
            task_id: sum(key[0] == task_id for key in grouped) for task_id in TASK_ORDER
        },
        "primary_config": PRIMARY_CONFIG,
        "crosscheck_config": CROSSCHECK_CONFIG,
        "crosscheck_seed": CROSSCHECK_SEED,
        "crosscheck_confirmed_rate": CROSSCHECK_RATE,
        "prompt_version": PROMPT_VERSION,
        "prompt_template_path": display_path(prompt_template_path),
        "prompt_template_sha256": sha256_file(prompt_template_path),
        "result_schema_path": display_path(result_schema_path),
        "result_schema_sha256": sha256_file(result_schema_path),
        "execution_engine": "collaboration",
        "codex_path": "",
        "codex_cli_version": "not_applicable_collaboration_agent",
        "input_paths": {
            "labels": display_path(LABELS_PATH),
            **{
                f"questions_{task_id}": display_path(path)
                for task_id, path in QUESTION_PATHS.items()
            },
        },
        "input_sha256": {
            "labels": sha256_file(LABELS_PATH),
            **{
                f"questions_{task_id}": sha256_file(path)
                for task_id, path in QUESTION_PATHS.items()
            },
        },
        "artifact_tree_sha256": combined_artifact_hash(artifact_hashes),
        "artifact_hashes": {
            f"{task_id}|{artifact_id}": digest
            for (task_id, artifact_id), digest in sorted(
                artifact_hashes.items(), key=lambda item: artifact_sort_key(item[0])
            )
        },
        "raw_selection_rule": "latest successful validated attempt per pass and artifact",
        "canonical_inputs_are_read_only": True,
    }
    freeze_json(output_dir / "execution_manifest.json", execution_manifest)


def write_population_manifest(path: Path, pairs: Sequence[Pair]) -> None:
    fields = (
        "pair_index",
        "task_id",
        "artifact_id",
        "artifact_path",
        "builder_id",
        "builder_family",
        "builder_effort",
        "build_trial_id",
        "question_id",
        "question_type",
        "gold_canonical_option",
        "source_requirement_id",
        "source_intention_id",
        "current_label",
        "current_label_source",
    )
    rows = []
    for index, pair in enumerate(pairs, start=1):
        source = pair.source
        rows.append(
            {
                "pair_index": index,
                "task_id": source["task_id"],
                "artifact_id": source["artifact_id"],
                "artifact_path": source["artifact_path"],
                "builder_id": source["builder_id"],
                "builder_family": source["builder_family"],
                "builder_effort": source["builder_effort"],
                "build_trial_id": source["build_trial_id"],
                "question_id": source["question_id"],
                "question_type": source["question_type"],
                "gold_canonical_option": source["gold_canonical_option"],
                "source_requirement_id": source["source_requirement_id"],
                "source_intention_id": source["source_intention_id"],
                "current_label": source["final_label"],
                "current_label_source": source["label_source"],
            }
        )
    write_csv(path, rows, fields)


def compute_artifact_hashes(
    grouped: Mapping[tuple[str, str], Sequence[Pair]],
) -> dict[tuple[str, str], str]:
    hashes = {}
    for key in ordered_artifact_keys(grouped):
        artifact_root = ROOT / grouped[key][0].source["artifact_path"]
        hashes[key] = tree_sha256(artifact_root)
    return hashes


def tree_sha256(root: Path) -> str:
    digest = hashlib.sha256()
    files = sorted(path for path in root.rglob("*") if path.is_file())
    for path in files:
        relative = path.relative_to(root).as_posix()
        digest.update(relative.encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(path.stat().st_size).encode("ascii"))
        digest.update(b"\0")
        digest.update(bytes.fromhex(sha256_file(path)))
        digest.update(b"\0")
    return digest.hexdigest()


def combined_artifact_hash(hashes: Mapping[tuple[str, str], str]) -> str:
    digest = hashlib.sha256()
    for key, value in sorted(hashes.items(), key=lambda item: artifact_sort_key(item[0])):
        digest.update("|".join(key).encode("utf-8"))
        digest.update(b"\0")
        digest.update(value.encode("ascii"))
        digest.update(b"\n")
    return digest.hexdigest()


def crosscheck_work(
    selection: Sequence[Mapping[str, str]],
) -> tuple[list[tuple[str, str]], dict[tuple[str, str], set[str]]]:
    artifacts = sorted(
        {(row["task_id"], row["artifact_id"]) for row in selection},
        key=artifact_sort_key,
    )
    questions: dict[tuple[str, str], set[str]] = defaultdict(set)
    for row in selection:
        questions[(row["task_id"], row["artifact_id"])].add(row["question_id"])
    return artifacts, questions


def prepare_agent_jobs(
    *,
    pass_config: Mapping[str, str],
    output_dir: Path,
    grouped: Mapping[tuple[str, str], Sequence[Pair]],
    artifact_keys: Sequence[tuple[str, str]],
    selected_questions: Mapping[tuple[str, str], set[str]] | None,
    prompt_template_path: Path,
    result_schema_path: Path,
) -> list[dict[str, Any]]:
    """Create immutable collaboration-agent assignments without launching a model."""

    jobs = []
    for processing_index, key in enumerate(artifact_keys, start=1):
        pairs = list(grouped[key])
        if selected_questions is not None:
            requested = selected_questions[key]
            pairs = [pair for pair in pairs if pair.source["question_id"] in requested]
            if {pair.source["question_id"] for pair in pairs} != requested:
                raise RuntimeError(f"agent question selection mismatch for {key}")
        existing = latest_successful_result(output_dir, pass_config["pass_id"], key[0], key[1])
        if existing is not None:
            jobs.append(
                agent_job_row(
                    pass_config=pass_config,
                    key=key,
                    processing_index=processing_index,
                    attempt_dir=existing.attempt_dir,
                    status="validated",
                )
            )
            continue

        raw_root = output_dir / "raw" / pass_config["pass_id"] / key[0] / key[1]
        raw_root.mkdir(parents=True, exist_ok=True)
        pending = latest_pending_agent_attempt(raw_root)
        if pending is None:
            attempt_dir = next_attempt_dir(raw_root)
            attempt_dir.mkdir()
            attempt_number = int(attempt_dir.name.split("-")[1])
            base_name = (
                f"full_audit_p{processing_index:02d}"
                if pass_config["pass_id"] == "primary"
                else f"full_audit_c{processing_index:02d}"
            )
            agent_task_name = (
                base_name if attempt_number == 1 else f"{base_name}_r{attempt_number:03d}"
            )
            prompt = render_prompt(prompt_template_path, pairs)
            assignment = render_agent_assignment(
                pass_config=pass_config,
                task_name=agent_task_name,
                attempt_dir=attempt_dir,
                prompt_path=attempt_dir / "prompt.md",
                result_schema_path=result_schema_path,
            )
            request = {
                "experiment_id": "full_conformance_audit_v1",
                "execution_engine": "collaboration.spawn_agent",
                "pass_id": pass_config["pass_id"],
                "task_id": key[0],
                "artifact_id": key[1],
                "artifact_path": pairs[0].source["artifact_path"],
                "question_ids": [pair.source["question_id"] for pair in pairs],
                "model_id": pass_config["model_id"],
                "reasoning_effort": pass_config["reasoning_effort"],
                "prompt_version": PROMPT_VERSION,
                "prompt_template_path": display_path(prompt_template_path),
                "prompt_template_sha256": sha256_file(prompt_template_path),
                "rendered_prompt_sha256": sha256_bytes(prompt.encode("utf-8")),
                "result_schema_path": display_path(result_schema_path),
                "result_schema_sha256": sha256_file(result_schema_path),
                "spec_version": SPEC_VERSION,
                "spec_path": display_path(output_dir / "SPEC.md"),
                "spec_sha256": sha256_file(output_dir / "SPEC.md"),
                "spec_role": (
                    "audit protocol documentation; prompt.md is the governing "
                    "model instruction"
                ),
                "agent_task_name": agent_task_name,
                "assignment_prompt_sha256": sha256_bytes(assignment.encode("utf-8")),
                "codex_cli_version": "not_applicable_collaboration_agent",
                "sandbox": "workspace-read_artifact_write-result-only",
                "started_at": utc_now(),
                "attempt_number": attempt_number,
            }
            write_new_json(attempt_dir / "request.json", request)
            write_new_text(attempt_dir / "prompt.md", prompt)
            write_new_text(attempt_dir / "assignment.md", assignment)
            pending = attempt_dir
        status = "result_ready" if (pending / "result.json").is_file() else "ready"
        jobs.append(
            agent_job_row(
                pass_config=pass_config,
                key=key,
                processing_index=processing_index,
                attempt_dir=pending,
                status=status,
            )
        )

    fields = (
        "processing_index",
        "pass_id",
        "task_id",
        "artifact_id",
        "agent_task_name",
        "model_id",
        "reasoning_effort",
        "status",
        "assignment_path",
        "prompt_path",
        "result_path",
    )
    write_csv(output_dir / f"agent_jobs_{pass_config['pass_id']}.csv", jobs, fields)
    ready = sum(row["status"] in {"ready", "result_ready"} for row in jobs)
    print(
        f"prepared {ready} {pass_config['pass_id']} collaboration-agent jobs "
        f"({len(jobs) - ready} already validated)",
        flush=True,
    )
    return jobs


def latest_pending_agent_attempt(raw_root: Path) -> Path | None:
    pending = []
    for attempt_dir in sorted(raw_root.glob("attempt-*")):
        if not attempt_dir.is_dir() or not (attempt_dir / "request.json").is_file():
            continue
        if not (attempt_dir / "completion.json").exists():
            pending.append(attempt_dir)
    return pending[-1] if pending else None


def latest_revalidatable_agent_attempt(raw_root: Path) -> Path | None:
    candidates = []
    for attempt_dir in sorted(raw_root.glob("attempt-*")):
        completion_path = attempt_dir / "completion.json"
        if (
            not attempt_dir.is_dir()
            or not completion_path.is_file()
            or not (attempt_dir / "result.json").is_file()
            or (attempt_dir / "revalidation.json").exists()
        ):
            continue
        completion = json.loads(completion_path.read_text(encoding="utf-8"))
        if completion.get("status") == "failed":
            candidates.append(attempt_dir)
    return candidates[-1] if candidates else None


def agent_job_row(
    *,
    pass_config: Mapping[str, str],
    key: tuple[str, str],
    processing_index: int,
    attempt_dir: Path,
    status: str,
) -> dict[str, Any]:
    request = json.loads((attempt_dir / "request.json").read_text(encoding="utf-8"))
    return {
        "processing_index": processing_index,
        "pass_id": pass_config["pass_id"],
        "task_id": key[0],
        "artifact_id": key[1],
        "agent_task_name": request["agent_task_name"],
        "model_id": request["model_id"],
        "reasoning_effort": request["reasoning_effort"],
        "status": status,
        "assignment_path": display_path(attempt_dir / "assignment.md"),
        "prompt_path": display_path(attempt_dir / "prompt.md"),
        "result_path": display_path(attempt_dir / "result.json"),
    }


def render_agent_assignment(
    *,
    pass_config: Mapping[str, str],
    task_name: str,
    attempt_dir: Path,
    prompt_path: Path,
    result_schema_path: Path,
) -> str:
    result_path = attempt_dir / "result.json"
    return "\n".join(
        [
            "# Isolated full-conformance source-adjudication assignment",
            "",
            f"- canonical task/session name: `{task_name}`",
            f"- requested model: `{pass_config['model_id']}`",
            f"- requested reasoning effort: `{pass_config['reasoning_effort']}`",
            f"- methodological prompt: `{display_path(prompt_path)}`",
            f"- result schema: `{display_path(result_schema_path)}`",
            f"- write the final JSON only to: `{display_path(result_path)}`",
            "",
            "Read the methodological prompt completely before inspecting source. "
            "Judge every listed question independently from live artifact code. Do not "
            "inspect label tables, prior audit outputs, cross-check manifests, or other "
            "agents' results. Use read-only shell commands for the artifact. The only "
            "authorized write is the designated result JSON, created with apply_patch. "
            "Do not modify runs_release or any other file. Validate that the JSON parses "
            "and matches the supplied schema, then finish with a concise confirmation.",
            "",
        ]
    )


def validate_agent_jobs(
    *,
    pass_config: Mapping[str, str],
    output_dir: Path,
    grouped: Mapping[tuple[str, str], Sequence[Pair]],
    selected_questions: Mapping[tuple[str, str], set[str]] | None,
) -> None:
    """Validate completed agent result files and append immutable completion records."""

    validated = 0
    waiting = []
    failures = []
    for key in ordered_artifact_keys(grouped):
        if selected_questions is not None and key not in selected_questions:
            continue
        expected_pairs = list(grouped[key])
        if selected_questions is not None:
            expected_pairs = [
                pair
                for pair in expected_pairs
                if pair.source["question_id"] in selected_questions[key]
            ]
        existing = latest_successful_result(output_dir, pass_config["pass_id"], key[0], key[1])
        if existing is not None:
            continue
        raw_root = output_dir / "raw" / pass_config["pass_id"] / key[0] / key[1]
        pending = latest_pending_agent_attempt(raw_root)
        revalidating = False
        if pending is None:
            pending = latest_revalidatable_agent_attempt(raw_root)
            revalidating = pending is not None
        if pending is None or not (pending / "result.json").is_file():
            waiting.append(key)
            continue
        request = json.loads((pending / "request.json").read_text(encoding="utf-8"))
        completion_name = "revalidation.json" if revalidating else "completion.json"
        try:
            if request["model_id"] != pass_config["model_id"]:
                raise RuntimeError("agent job model id drift")
            if request["reasoning_effort"] != pass_config["reasoning_effort"]:
                raise RuntimeError("agent job reasoning effort drift")
            payload = json.loads((pending / "result.json").read_text(encoding="utf-8"))
            normalized = validate_payload(
                payload=payload,
                pairs=expected_pairs,
                artifact_root=ROOT / expected_pairs[0].source["artifact_path"],
            )
            if normalized != payload:
                write_new_json(pending / "normalized_result.json", normalized)
            normalized_path = pending / "normalized_result.json"
            provenance = {
                **request,
                "completed_at": utc_now(),
                "duration_seconds": elapsed_seconds(request["started_at"]),
                "returncode": "not_applicable",
                "timed_out": False,
                "session_id": request["agent_task_name"],
                "session_id_source": "collaboration.spawn_agent canonical task name",
                "model_id_source": "explicit parent collaboration.spawn_agent request",
                "result_sha256": sha256_file(pending / "result.json"),
                "selected_result_sha256": sha256_file(
                    normalized_path if normalized_path.is_file() else pending / "result.json"
                ),
                "status": "validated",
                "revalidated_after_validator_change": revalidating,
            }
        except Exception as error:  # noqa: BLE001 - preserve failed attempt
            write_new_json(
                pending / completion_name,
                {
                    **request,
                    "completed_at": utc_now(),
                    "duration_seconds": elapsed_seconds(request["started_at"]),
                    "returncode": "not_applicable",
                    "timed_out": False,
                    "session_id": request["agent_task_name"],
                    "status": "failed",
                    "error_type": type(error).__name__,
                    "error": str(error),
                },
            )
            failures.append((key, str(error)))
            continue
        write_new_json(pending / completion_name, provenance)
        try:
            create_integrity_attestation(
                output_dir=output_dir,
                attempt_dir=pending,
                pass_id=pass_config["pass_id"],
                task_id=key[0],
                artifact_id=key[1],
                expected_pairs=expected_pairs,
            )
        except Exception as error:  # noqa: BLE001 - completion remains immutable
            failures.append((key, f"integrity attestation failed: {error}"))
            continue
        validated += 1
    print(
        f"validated {validated} new {pass_config['pass_id']} agent results; "
        f"waiting={len(waiting)} failures={len(failures)}",
        flush=True,
    )
    if failures:
        raise RuntimeError(
            "agent result validation failures:\n"
            + "\n".join(f"{key}: {error}" for key, error in failures)
        )


def render_prompt(prompt_template_path: Path, pairs: Sequence[Pair]) -> str:
    template = prompt_template_path.read_text(encoding="utf-8").rstrip()
    first = pairs[0].source
    payload = []
    for pair in pairs:
        question = pair.question
        payload.append(
            {
                "question_id": question["question_id"],
                "question_type": question["question_type"],
                "prompt": normalize_space(question["prompt"]),
                "options": question["options"],
                "gold_option": question["correct_option"],
                "source_trace": question["source_trace"],
            }
        )
    return (
        f"{template}\n\n"
        "## Assigned artifact\n\n"
        f"- task_id: `{first['task_id']}`\n"
        f"- artifact_id: `{first['artifact_id']}`\n"
        f"- artifact root: `{first['artifact_path']}`\n"
        f"- requested questions: {len(payload)}\n\n"
        "## Canonical question payload\n\n"
        "```json\n"
        f"{json.dumps(payload, indent=2, ensure_ascii=False)}\n"
        "```\n"
    )


def next_attempt_dir(raw_root: Path) -> Path:
    indices = []
    for path in raw_root.glob("attempt-*"):
        if not path.is_dir():
            continue
        match = re.fullmatch(r"attempt-(\d{3})", path.name)
        if match:
            indices.append(int(match.group(1)))
    return raw_root / f"attempt-{max(indices, default=0) + 1:03d}"


def validate_payload(
    *,
    payload: dict[str, Any],
    pairs: Sequence[Pair],
    artifact_root: Path,
) -> dict[str, Any]:
    expected_task = pairs[0].source["task_id"]
    expected_artifact = pairs[0].source["artifact_id"]
    if payload.get("task_id") != expected_task:
        raise RuntimeError(
            f"payload task mismatch: {payload.get('task_id')!r} != {expected_task!r}"
        )
    if payload.get("artifact_id") != expected_artifact:
        raise RuntimeError(
            f"payload artifact mismatch: {payload.get('artifact_id')!r} != {expected_artifact!r}"
        )
    rows = payload.get("rows")
    if not isinstance(rows, list):
        raise RuntimeError("payload rows must be a list")
    expected_by_question = {pair.source["question_id"]: pair for pair in pairs}
    received_ids = [row.get("question_id") for row in rows if isinstance(row, dict)]
    if len(rows) != len(expected_by_question) or set(received_ids) != set(expected_by_question):
        raise RuntimeError(
            f"payload question set mismatch: received={received_ids}, "
            f"expected={sorted(expected_by_question)}"
        )
    if len(received_ids) != len(set(received_ids)):
        raise RuntimeError("payload contains duplicate question rows")

    normalized_rows = []
    for raw_row in rows:
        if not isinstance(raw_row, dict):
            raise RuntimeError("payload row must be an object")
        pair = expected_by_question[raw_row["question_id"]]
        verdict = raw_row.get("verdict")
        actual_option = raw_row.get("actual_option")
        confidence = raw_row.get("confidence")
        if verdict not in VERDICTS:
            raise RuntimeError(f"invalid verdict: {verdict!r}")
        if actual_option not in ACTUAL_OPTIONS:
            raise RuntimeError(f"invalid actual option: {actual_option!r}")
        if confidence not in CONFIDENCES:
            raise RuntimeError(f"invalid confidence: {confidence!r}")
        if raw_row.get("runtime_reachability_checked") is not True:
            raise RuntimeError(f"reachability not checked for {raw_row['question_id']}")
        gold = pair.question["correct_option"]
        if verdict == "confirmed_gold" and actual_option != gold:
            raise RuntimeError(f"confirmed_gold must select {gold} for {raw_row['question_id']}")
        if verdict == "non_gold" and (
            actual_option not in {"A", "B", "C", "D"} or actual_option == gold
        ):
            raise RuntimeError(
                f"non_gold must select one non-gold option for {raw_row['question_id']}"
            )
        evidence_files = [
            normalize_relative_path(path)
            for path in normalize_string_list(raw_row.get("evidence_files"), "evidence_files")
        ]
        evidence_files = list(dict.fromkeys(evidence_files))
        evidence_symbols = normalize_string_list(
            raw_row.get("evidence_symbols"), "evidence_symbols"
        )
        spans = validate_spans(
            raw_row.get("evidence_spans"),
            artifact_root=artifact_root,
            question_id=raw_row["question_id"],
        )
        span_files = {span["file"] for span in spans}
        if not span_files.issubset(set(evidence_files)):
            raise RuntimeError(
                f"span files missing from evidence_files for {raw_row['question_id']}"
            )
        for relative in evidence_files:
            resolve_evidence_file(artifact_root, relative)
        rationale = raw_row.get("rationale")
        if not isinstance(rationale, str) or not rationale.strip():
            raise RuntimeError(f"empty rationale for {raw_row['question_id']}")
        normalized_rows.append(
            {
                "question_id": raw_row["question_id"],
                "verdict": verdict,
                "actual_option": actual_option,
                "confidence": confidence,
                "runtime_reachability_checked": True,
                "evidence_files": evidence_files,
                "evidence_symbols": evidence_symbols,
                "evidence_spans": spans,
                "rationale": normalize_space(rationale),
            }
        )
    normalized_rows.sort(key=lambda row: row["question_id"])
    return {
        "artifact_id": expected_artifact,
        "task_id": expected_task,
        "rows": normalized_rows,
    }


def normalize_string_list(value: Any, field: str) -> list[str]:
    if not isinstance(value, list) or not value:
        raise RuntimeError(f"{field} must be a non-empty list")
    normalized = []
    for item in value:
        if not isinstance(item, str) or not item.strip():
            raise RuntimeError(f"{field} contains an empty/non-string item")
        normalized.append(item.strip())
    return list(dict.fromkeys(normalized))


def validate_spans(
    value: Any,
    *,
    artifact_root: Path,
    question_id: str,
) -> list[dict[str, Any]]:
    if not isinstance(value, list) or not value:
        raise RuntimeError(f"evidence_spans must be non-empty for {question_id}")
    normalized = []
    for raw_span in value:
        if not isinstance(raw_span, dict):
            raise RuntimeError(f"non-object span for {question_id}")
        relative = raw_span.get("file")
        start = raw_span.get("start_line")
        end = raw_span.get("end_line")
        if not isinstance(relative, str) or not relative.strip():
            raise RuntimeError(f"span has invalid file for {question_id}")
        if not isinstance(start, int) or isinstance(start, bool) or start < 1:
            raise RuntimeError(f"span has invalid start_line for {question_id}")
        if not isinstance(end, int) or isinstance(end, bool) or end < start:
            raise RuntimeError(f"span has invalid end_line for {question_id}")
        path = resolve_evidence_file(artifact_root, relative)
        line_count = count_lines(path)
        if end > line_count:
            raise RuntimeError(
                f"span {relative}:{start}-{end} exceeds {line_count} lines for {question_id}"
            )
        normalized.append(
            {"file": normalize_relative_path(relative), "start_line": start, "end_line": end}
        )
    seen = set()
    deduplicated = []
    for span in normalized:
        key = (span["file"], span["start_line"], span["end_line"])
        if key not in seen:
            deduplicated.append(span)
            seen.add(key)
    return deduplicated


def normalize_relative_path(raw: str) -> str:
    candidate = PurePosixPath(raw.strip().replace("\\", "/"))
    if candidate.is_absolute() or ".." in candidate.parts or not candidate.parts:
        raise RuntimeError(f"invalid artifact-relative evidence path: {raw!r}")
    if candidate.parts[0] == ".":
        candidate = PurePosixPath(*candidate.parts[1:])
    if not candidate.parts:
        raise RuntimeError(f"invalid artifact-relative evidence path: {raw!r}")
    return candidate.as_posix()


def resolve_evidence_file(artifact_root: Path, raw: str) -> Path:
    relative = normalize_relative_path(raw)
    root = artifact_root.resolve()
    path = (root / relative).resolve()
    if not path.is_relative_to(root):
        raise RuntimeError(f"evidence path escapes artifact root: {raw!r}")
    if not path.is_file():
        raise RuntimeError(f"evidence file does not exist: {raw!r}")
    return path


def count_lines(path: Path) -> int:
    with path.open("rb") as handle:
        return sum(1 for _ in handle)


def validated_completion_path(attempt_dir: Path) -> Path | None:
    """Return the immutable record that most recently validated an attempt."""

    for name in ("revalidation.json", "completion.json"):
        path = attempt_dir / name
        if not path.is_file():
            continue
        record = json.loads(path.read_text(encoding="utf-8"))
        if record.get("status") == "validated":
            return path
    return None


def create_integrity_attestation(
    *,
    output_dir: Path,
    attempt_dir: Path,
    pass_id: str,
    task_id: str,
    artifact_id: str,
    expected_pairs: Sequence[Pair],
) -> dict[str, Any]:
    """Bind a validated attempt to exact immutable bytes without rewriting history."""

    attestation_path = attempt_dir / "integrity_attestation.json"
    if attestation_path.exists():
        return verify_integrity_attestation(
            output_dir=output_dir,
            attempt_dir=attempt_dir,
            pass_id=pass_id,
            task_id=task_id,
            artifact_id=artifact_id,
        )[0]

    completion_path = validated_completion_path(attempt_dir)
    if completion_path is None:
        raise RuntimeError(f"attempt has no validated completion record: {attempt_dir}")
    request_path = attempt_dir / "request.json"
    prompt_path = attempt_dir / "prompt.md"
    assignment_path = attempt_dir / "assignment.md"
    result_path = attempt_dir / "result.json"
    required = (request_path, prompt_path, assignment_path, result_path)
    missing = [path for path in required if not path.is_file()]
    if missing:
        raise RuntimeError(f"validated attempt lacks immutable evidence files: {missing}")

    request = json.loads(request_path.read_text(encoding="utf-8"))
    completion = json.loads(completion_path.read_text(encoding="utf-8"))
    for key, value in request.items():
        if completion.get(key) != value:
            raise RuntimeError(
                f"completion/request provenance mismatch for {attempt_dir}: {key}"
            )
    if request.get("pass_id") != pass_id:
        raise RuntimeError(f"request pass mismatch for {attempt_dir}")
    if request.get("task_id") != task_id or request.get("artifact_id") != artifact_id:
        raise RuntimeError(f"request artifact identity mismatch for {attempt_dir}")
    expected_config = PRIMARY_CONFIG if pass_id == "primary" else CROSSCHECK_CONFIG
    for field in ("model_id", "reasoning_effort", "pass_id"):
        if request.get(field) != expected_config[field]:
            raise RuntimeError(f"request {field} drift for {attempt_dir}")
    if request.get("prompt_version") != PROMPT_VERSION:
        raise RuntimeError(f"request prompt version drift for {attempt_dir}")
    if sha256_file(prompt_path) != request.get("rendered_prompt_sha256"):
        raise RuntimeError(f"rendered prompt hash mismatch for {attempt_dir}")
    if sha256_file(assignment_path) != request.get("assignment_prompt_sha256"):
        raise RuntimeError(f"assignment hash mismatch for {attempt_dir}")

    requested_ids = request.get("question_ids")
    if not isinstance(requested_ids, list) or not requested_ids:
        raise RuntimeError(f"request has invalid question_ids for {attempt_dir}")
    pair_by_id = {pair.source["question_id"]: pair for pair in expected_pairs}
    if set(requested_ids) != set(pair_by_id):
        raise RuntimeError(f"request question set mismatch for {attempt_dir}")
    ordered_pairs = [pair_by_id[question_id] for question_id in requested_ids]
    normalized_path = attempt_dir / "normalized_result.json"
    selected_result_path = normalized_path if normalized_path.is_file() else result_path
    payload = json.loads(selected_result_path.read_text(encoding="utf-8"))
    normalized = validate_payload(
        payload=payload,
        pairs=ordered_pairs,
        artifact_root=ROOT / ordered_pairs[0].source["artifact_path"],
    )
    if normalized != payload:
        raise RuntimeError(f"selected result is not canonical for {attempt_dir}")

    raw_result_sha256 = sha256_file(result_path)
    selected_result_sha256 = sha256_file(selected_result_path)
    completion_hashes = (
        completion.get("result_sha256"),
        completion.get("selected_result_sha256"),
    )
    completion_time_hash_present = all(
        isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value)
        for value in completion_hashes
    )
    if any(value not in (None, "") for value in completion_hashes) and not (
        completion_time_hash_present
    ):
        raise RuntimeError(f"partial or invalid completion-time hashes for {attempt_dir}")
    if completion_time_hash_present:
        if completion_hashes[0] != raw_result_sha256:
            raise RuntimeError(f"completion-time result hash drift for {attempt_dir}")
        if completion_hashes[1] != selected_result_sha256:
            raise RuntimeError(f"completion-time selected-result hash drift for {attempt_dir}")
        result_hash_source = "completion_record_and_parent_verified"
    else:
        result_hash_source = "post_hoc_parent_attestation"

    spec_path = output_dir / "SPEC.md"
    if not spec_path.is_file():
        raise RuntimeError(f"missing audit protocol specification: {spec_path}")
    request_spec_sha256 = request.get("spec_sha256")
    if request_spec_sha256:
        if request_spec_sha256 != sha256_file(spec_path):
            raise RuntimeError(f"request-bound specification hash drift for {attempt_dir}")
        spec_binding_source = "request_record"
    else:
        spec_binding_source = "post_hoc_protocol_document"

    attestation = {
        "attestation_version": INTEGRITY_ATTESTATION_VERSION,
        "attestation_type": "append_only_parent_integrity_attestation",
        "attested_at": utc_now(),
        "status": "validated",
        "pass_id": pass_id,
        "task_id": task_id,
        "artifact_id": artifact_id,
        "attempt_path": display_path(attempt_dir),
        "session_id": completion.get("session_id"),
        "session_id_source": completion.get(
            "session_id_source",
            "collaboration.spawn_agent canonical task name",
        ),
        "model_id": completion.get("model_id"),
        "model_id_source": completion.get(
            "model_id_source",
            "explicit parent collaboration.spawn_agent request",
        ),
        "completion_record_path": display_path(completion_path),
        "completion_record_sha256": sha256_file(completion_path),
        "request_path": display_path(request_path),
        "request_sha256": sha256_file(request_path),
        "prompt_path": display_path(prompt_path),
        "prompt_sha256": sha256_file(prompt_path),
        "assignment_path": display_path(assignment_path),
        "assignment_sha256": sha256_file(assignment_path),
        "result_path": display_path(result_path),
        "result_sha256": raw_result_sha256,
        "selected_result_path": display_path(selected_result_path),
        "selected_result_sha256": selected_result_sha256,
        "completion_time_result_hash_present": completion_time_hash_present,
        "result_hash_source": result_hash_source,
        "question_ids": requested_ids,
        "validated_question_count": len(requested_ids),
        "governing_model_instruction": "prompt.md",
        "governing_prompt_sha256": request["rendered_prompt_sha256"],
        "spec_version": request.get("spec_version", SPEC_VERSION),
        "spec_path": display_path(spec_path),
        "spec_sha256": sha256_file(spec_path),
        "spec_binding_source": spec_binding_source,
        "spec_role": (
            "protocol documentation; the per-attempt prompt.md is the exact "
            "instruction that governed model output"
        ),
    }
    original_completion_path = attempt_dir / "completion.json"
    if completion_path.name == "revalidation.json" and original_completion_path.is_file():
        attestation["prior_completion_record_path"] = display_path(original_completion_path)
        attestation["prior_completion_record_sha256"] = sha256_file(
            original_completion_path
        )
    write_new_json(attestation_path, attestation)
    return verify_integrity_attestation(
        output_dir=output_dir,
        attempt_dir=attempt_dir,
        pass_id=pass_id,
        task_id=task_id,
        artifact_id=artifact_id,
    )[0]


def verify_integrity_attestation(
    *,
    output_dir: Path,
    attempt_dir: Path,
    pass_id: str,
    task_id: str,
    artifact_id: str,
) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any], Path]:
    """Fail closed unless an attestation binds every selected evidence byte."""

    attestation_path = attempt_dir / "integrity_attestation.json"
    if not attestation_path.is_file():
        raise RuntimeError(
            f"validated attempt lacks integrity attestation; run --phase "
            f"attest-integrity: {attempt_dir}"
        )
    attestation = json.loads(attestation_path.read_text(encoding="utf-8"))
    expected_identity = {
        "attestation_version": INTEGRITY_ATTESTATION_VERSION,
        "status": "validated",
        "pass_id": pass_id,
        "task_id": task_id,
        "artifact_id": artifact_id,
        "attempt_path": display_path(attempt_dir),
    }
    for field, expected in expected_identity.items():
        if attestation.get(field) != expected:
            raise RuntimeError(
                f"integrity attestation identity mismatch for {attempt_dir}: {field}"
            )

    completion_path = validated_completion_path(attempt_dir)
    if completion_path is None:
        raise RuntimeError(f"attested attempt lost validated completion: {attempt_dir}")
    request_path = attempt_dir / "request.json"
    prompt_path = attempt_dir / "prompt.md"
    assignment_path = attempt_dir / "assignment.md"
    result_path = attempt_dir / "result.json"
    normalized_path = attempt_dir / "normalized_result.json"
    selected_result_path = normalized_path if normalized_path.is_file() else result_path
    file_bindings = {
        "completion_record": completion_path,
        "request": request_path,
        "prompt": prompt_path,
        "assignment": assignment_path,
        "result": result_path,
        "selected_result": selected_result_path,
    }
    for prefix, path in file_bindings.items():
        if attestation.get(f"{prefix}_path") != display_path(path):
            raise RuntimeError(f"attested {prefix} path drift for {attempt_dir}")
        if not path.is_file() or attestation.get(f"{prefix}_sha256") != sha256_file(path):
            raise RuntimeError(f"attested {prefix} hash drift for {attempt_dir}")

    completion = json.loads(completion_path.read_text(encoding="utf-8"))
    request = json.loads(request_path.read_text(encoding="utf-8"))
    if completion.get("status") != "validated":
        raise RuntimeError(f"attested completion is not validated: {attempt_dir}")
    for key, value in request.items():
        if completion.get(key) != value:
            raise RuntimeError(
                f"attested completion/request mismatch for {attempt_dir}: {key}"
            )
    if sha256_file(prompt_path) != request.get("rendered_prompt_sha256"):
        raise RuntimeError(f"attested prompt/request mismatch for {attempt_dir}")
    if sha256_file(assignment_path) != request.get("assignment_prompt_sha256"):
        raise RuntimeError(f"attested assignment/request mismatch for {attempt_dir}")

    completion_hash_present = attestation.get("completion_time_result_hash_present")
    expected_source = (
        "completion_record_and_parent_verified"
        if completion_hash_present
        else "post_hoc_parent_attestation"
    )
    if attestation.get("result_hash_source") != expected_source:
        raise RuntimeError(f"attested result-hash source mismatch for {attempt_dir}")
    recorded_hashes = (
        completion.get("result_sha256"),
        completion.get("selected_result_sha256"),
    )
    if completion_hash_present:
        if recorded_hashes != (
            attestation["result_sha256"],
            attestation["selected_result_sha256"],
        ):
            raise RuntimeError(f"completion-time result hashes drifted for {attempt_dir}")
    elif any(value not in (None, "") for value in recorded_hashes):
        raise RuntimeError(f"post-hoc hash source contradicts completion for {attempt_dir}")

    spec_path = output_dir / "SPEC.md"
    if attestation.get("spec_version") != SPEC_VERSION:
        raise RuntimeError(f"attested specification version drift for {attempt_dir}")
    if attestation.get("spec_path") != display_path(spec_path):
        raise RuntimeError(f"attested specification path drift for {attempt_dir}")
    if not spec_path.is_file() or attestation.get("spec_sha256") != sha256_file(spec_path):
        raise RuntimeError(f"attested specification bytes drifted for {attempt_dir}")
    if attestation.get("governing_prompt_sha256") != request.get(
        "rendered_prompt_sha256"
    ):
        raise RuntimeError(f"attested governing prompt drift for {attempt_dir}")

    payload = json.loads(selected_result_path.read_text(encoding="utf-8"))
    if payload.get("task_id") != task_id or payload.get("artifact_id") != artifact_id:
        raise RuntimeError(f"attested payload identity drift for {attempt_dir}")
    payload_ids = [row.get("question_id") for row in payload.get("rows", [])]
    if payload_ids != request.get("question_ids"):
        raise RuntimeError(f"attested payload question order drift for {attempt_dir}")
    if attestation.get("question_ids") != payload_ids:
        raise RuntimeError(f"attested question ids drift for {attempt_dir}")
    if attestation.get("validated_question_count") != len(payload_ids):
        raise RuntimeError(f"attested question count drift for {attempt_dir}")
    return attestation, completion, payload, selected_result_path


def integrity_manifest_entries(
    *,
    output_dir: Path,
    grouped: Mapping[tuple[str, str], Sequence[Pair]],
    create_missing: bool,
) -> tuple[list[dict[str, Any]], int]:
    entries: list[dict[str, Any]] = []
    failed_attempt_count = 0
    for pass_id in ("primary", "crosscheck"):
        for task_id, artifact_id in ordered_artifact_keys(grouped):
            raw_root = output_dir / "raw" / pass_id / task_id / artifact_id
            if not raw_root.is_dir():
                continue
            for attempt_dir in sorted(raw_root.glob("attempt-*")):
                completion_path = validated_completion_path(attempt_dir)
                if completion_path is None:
                    records = [
                        path
                        for path in (
                            attempt_dir / "completion.json",
                            attempt_dir / "revalidation.json",
                        )
                        if path.is_file()
                    ]
                    if records and any(
                        json.loads(path.read_text(encoding="utf-8")).get("status")
                        == "failed"
                        for path in records
                    ):
                        failed_attempt_count += 1
                    continue
                request = json.loads((attempt_dir / "request.json").read_text(encoding="utf-8"))
                requested_ids = request.get("question_ids", [])
                pairs_by_id = {
                    pair.source["question_id"]: pair for pair in grouped[(task_id, artifact_id)]
                }
                if any(question_id not in pairs_by_id for question_id in requested_ids):
                    raise RuntimeError(f"unknown requested question in {attempt_dir}")
                expected_pairs = [pairs_by_id[question_id] for question_id in requested_ids]
                if create_missing:
                    create_integrity_attestation(
                        output_dir=output_dir,
                        attempt_dir=attempt_dir,
                        pass_id=pass_id,
                        task_id=task_id,
                        artifact_id=artifact_id,
                        expected_pairs=expected_pairs,
                    )
                attestation, _, _, _ = verify_integrity_attestation(
                    output_dir=output_dir,
                    attempt_dir=attempt_dir,
                    pass_id=pass_id,
                    task_id=task_id,
                    artifact_id=artifact_id,
                )
                entries.append(
                    {
                        "pass_id": pass_id,
                        "task_id": task_id,
                        "artifact_id": artifact_id,
                        "attempt_path": display_path(attempt_dir),
                        "integrity_attestation_path": display_path(
                            attempt_dir / "integrity_attestation.json"
                        ),
                        "integrity_attestation_sha256": sha256_file(
                            attempt_dir / "integrity_attestation.json"
                        ),
                        "completion_time_result_hash_present": attestation[
                            "completion_time_result_hash_present"
                        ],
                        "result_hash_source": attestation["result_hash_source"],
                        "selected_result_sha256": attestation[
                            "selected_result_sha256"
                        ],
                        "validated_question_count": attestation[
                            "validated_question_count"
                        ],
                    }
                )
    entries.sort(
        key=lambda row: (
            0 if row["pass_id"] == "primary" else 1,
            *artifact_sort_key((row["task_id"], row["artifact_id"])),
            row["attempt_path"],
        )
    )
    return entries, failed_attempt_count


def attest_validated_results(
    *,
    output_dir: Path,
    grouped: Mapping[tuple[str, str], Sequence[Pair]],
) -> None:
    """Create the closed, append-only integrity ledger for all successful attempts."""

    entries, failed_attempt_count = integrity_manifest_entries(
        output_dir=output_dir,
        grouped=grouped,
        create_missing=True,
    )
    if len(entries) != 2 * EXPECTED_ARTIFACTS:
        raise RuntimeError(
            f"expected {2 * EXPECTED_ARTIFACTS} validated attempts, found {len(entries)}"
        )
    pass_counts = Counter(entry["pass_id"] for entry in entries)
    if pass_counts != Counter({"primary": EXPECTED_ARTIFACTS, "crosscheck": EXPECTED_ARTIFACTS}):
        raise RuntimeError(f"integrity pass counts are incomplete: {pass_counts}")
    source_counts = Counter(entry["result_hash_source"] for entry in entries)
    attested_times = [
        json.loads(
            (ROOT / entry["integrity_attestation_path"]).read_text(encoding="utf-8")
        )["attested_at"]
        for entry in entries
    ]
    manifest = {
        "manifest_version": INTEGRITY_ATTESTATION_VERSION,
        "closed_at": max(attested_times),
        "validated_attempt_count": len(entries),
        "failed_attempt_count": failed_attempt_count,
        "pass_counts": dict(pass_counts),
        "result_hash_source_counts": dict(source_counts),
        "spec_version": SPEC_VERSION,
        "spec_path": display_path(output_dir / "SPEC.md"),
        "spec_sha256": sha256_file(output_dir / "SPEC.md"),
        "spec_role": (
            "post-hoc protocol integrity for historical attempts; per-attempt "
            "prompt.md remains the governing model instruction"
        ),
        "attestation_script": display_path(Path(__file__).resolve()),
        "attestation_script_sha256": sha256_file(Path(__file__).resolve()),
        "entries": entries,
    }
    freeze_json(output_dir / "integrity_attestation_manifest.json", manifest)
    print(
        f"attested {len(entries)} validated attempts; "
        f"hash sources={dict(source_counts)} failed attempts preserved={failed_attempt_count}",
        flush=True,
    )


def verify_integrity_manifest(
    *,
    output_dir: Path,
    grouped: Mapping[tuple[str, str], Sequence[Pair]],
) -> dict[str, Any]:
    manifest_path = output_dir / "integrity_attestation_manifest.json"
    if not manifest_path.is_file():
        raise RuntimeError("missing closed integrity attestation manifest")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    entries, failed_attempt_count = integrity_manifest_entries(
        output_dir=output_dir,
        grouped=grouped,
        create_missing=False,
    )
    if manifest.get("manifest_version") != INTEGRITY_ATTESTATION_VERSION:
        raise RuntimeError("integrity manifest version drift")
    if manifest.get("entries") != entries:
        raise RuntimeError("integrity manifest entry set or hashes drifted")
    if manifest.get("validated_attempt_count") != len(entries):
        raise RuntimeError("integrity manifest validated-attempt count drifted")
    if manifest.get("failed_attempt_count") != failed_attempt_count:
        raise RuntimeError("integrity manifest failed-attempt count drifted")
    if manifest.get("pass_counts") != dict(Counter(entry["pass_id"] for entry in entries)):
        raise RuntimeError("integrity manifest pass counts drifted")
    if manifest.get("result_hash_source_counts") != dict(
        Counter(entry["result_hash_source"] for entry in entries)
    ):
        raise RuntimeError("integrity manifest hash-source counts drifted")
    if manifest.get("spec_version") != SPEC_VERSION:
        raise RuntimeError("integrity manifest specification version drifted")
    if manifest.get("spec_sha256") != sha256_file(output_dir / "SPEC.md"):
        raise RuntimeError("integrity manifest specification hash drifted")
    return manifest


def latest_successful_result(
    output_dir: Path,
    pass_id: str,
    task_id: str,
    artifact_id: str,
) -> SessionResult | None:
    raw_root = output_dir / "raw" / pass_id / task_id / artifact_id
    if not raw_root.is_dir():
        return None
    candidates = []
    for attempt_dir in sorted(raw_root.glob("attempt-*")):
        completion_path = validated_completion_path(attempt_dir)
        if completion_path is None:
            continue
        attestation, completion, payload, _ = verify_integrity_attestation(
            output_dir=output_dir,
            attempt_dir=attempt_dir,
            pass_id=pass_id,
            task_id=task_id,
            artifact_id=artifact_id,
        )
        provenance = {
            **completion,
            "session_id_source": completion.get(
                "session_id_source",
                "collaboration.spawn_agent canonical task name",
            ),
            "model_id_source": completion.get(
                "model_id_source",
                "explicit parent collaboration.spawn_agent request",
            ),
            "result_sha256": attestation["result_sha256"],
            "selected_result_sha256": attestation["selected_result_sha256"],
            "result_hash_source": attestation["result_hash_source"],
            "integrity_attestation_sha256": sha256_file(
                attempt_dir / "integrity_attestation.json"
            ),
            "spec_version": attestation["spec_version"],
            "spec_sha256": attestation["spec_sha256"],
            "spec_binding_source": attestation["spec_binding_source"],
        }
        candidates.append(
            SessionResult(
                pass_id=pass_id,
                artifact_id=artifact_id,
                task_id=task_id,
                attempt_dir=attempt_dir,
                payload=payload,
                provenance=provenance,
            )
        )
    return candidates[-1] if candidates else None


def require_complete_primary(
    output_dir: Path,
    grouped: Mapping[tuple[str, str], Sequence[Pair]],
) -> None:
    missing = [
        key
        for key in ordered_artifact_keys(grouped)
        if latest_successful_result(output_dir, "primary", key[0], key[1]) is None
    ]
    if missing:
        raise RuntimeError(f"primary pass incomplete for {len(missing)} artifacts: {missing}")


def freeze_crosscheck_selection(
    output_dir: Path,
    pairs: Sequence[Pair],
    grouped: Mapping[tuple[str, str], Sequence[Pair]],
) -> list[dict[str, str]]:
    primary_by_key = load_pass_rows(output_dir, grouped, "primary")
    rows, sampling_manifest = derive_crosscheck_selection(pairs, primary_by_key)
    path = output_dir / "crosscheck_manifest.csv"
    rendered = render_csv(rows, CROSSCHECK_SELECTION_FIELDS)
    sampling_manifest["selection_hash"] = sha256_bytes(rendered.encode("utf-8"))
    freeze_text(path, rendered)
    freeze_json(output_dir / "crosscheck_sampling_manifest.json", sampling_manifest)
    return rows


CROSSCHECK_SELECTION_FIELDS = (
    "selection_index",
    "task_id",
    "artifact_id",
    "question_id",
    "builder_family",
    "primary_verdict",
    "selection_reason",
    "sample_seed",
    "confirmed_sample_rate",
)


def derive_crosscheck_selection(
    pairs: Sequence[Pair],
    primary_by_key: Mapping[tuple[str, str, str], Mapping[str, Any]],
) -> tuple[list[dict[str, str]], dict[str, Any]]:
    """Derive the complete cross-check design from immutable first-pass rows."""

    expected_keys = {pair.key for pair in pairs}
    if set(primary_by_key) != expected_keys:
        raise RuntimeError("cannot derive crosscheck selection from incomplete primary rows")
    confirmed = [
        pair for pair in pairs if primary_by_key[pair.key]["row"]["verdict"] == "confirmed_gold"
    ]
    flagged = [
        pair for pair in pairs if primary_by_key[pair.key]["row"]["verdict"] != "confirmed_gold"
    ]
    sample_keys, allocation = stratified_confirmed_sample(confirmed)
    selected_reasons: dict[tuple[str, str, str], set[str]] = defaultdict(set)
    for pair in flagged:
        selected_reasons[pair.key].add("primary_flag")
    for key in sample_keys:
        selected_reasons[key].add("confirmed_10pct_stratified_sample")
    for key in KNOWN_REGRESSIONS:
        selected_reasons[key].add("known_regression")

    rows = []
    pair_by_key = {pair.key: pair for pair in pairs}
    for index, key in enumerate(sorted(selected_reasons, key=pair_sort_key), start=1):
        pair = pair_by_key[key]
        rows.append(
            {
                "selection_index": str(index),
                "task_id": key[0],
                "artifact_id": key[1],
                "question_id": key[2],
                "builder_family": pair.source["builder_family"],
                "primary_verdict": primary_by_key[key]["row"]["verdict"],
                "selection_reason": "|".join(sorted(selected_reasons[key])),
                "sample_seed": str(CROSSCHECK_SEED),
                "confirmed_sample_rate": f"{CROSSCHECK_RATE:.2f}",
            }
        )
    sample_count = len(sample_keys)
    expected_sample_count = math.ceil(len(confirmed) * CROSSCHECK_RATE)
    if sample_count != expected_sample_count:
        raise RuntimeError(
            f"confirmed sample size mismatch: {sample_count} != {expected_sample_count}"
        )
    return rows, {
        "seed": CROSSCHECK_SEED,
        "confirmed_population": len(confirmed),
        "confirmed_sample_rate": CROSSCHECK_RATE,
        "confirmed_sample_count": sample_count,
        "primary_flag_count": len(flagged),
        "known_regression_count": len(KNOWN_REGRESSIONS),
        "union_pair_count": len(rows),
        "stratum_allocation": {
            f"{task_id}|{family}": count for (task_id, family), count in sorted(allocation.items())
        },
    }


def stratified_confirmed_sample(
    confirmed: Sequence[Pair],
) -> tuple[set[tuple[str, str, str]], dict[tuple[str, str], int]]:
    strata: dict[tuple[str, str], list[Pair]] = defaultdict(list)
    for pair in confirmed:
        strata[(pair.source["task_id"], pair.source["builder_family"])].append(pair)
    target = math.ceil(len(confirmed) * CROSSCHECK_RATE)
    exact = {stratum: len(members) * CROSSCHECK_RATE for stratum, members in strata.items()}
    allocation = {stratum: math.floor(value) for stratum, value in exact.items()}
    remaining = target - sum(allocation.values())
    remainder_order = sorted(
        strata,
        key=lambda stratum: (
            -(exact[stratum] - allocation[stratum]),
            TASK_ORDER.index(stratum[0]),
            stratum[1],
        ),
    )
    for stratum in remainder_order[:remaining]:
        allocation[stratum] += 1
    if sum(allocation.values()) != target:
        raise RuntimeError("stratified allocation failed to reach target")
    selected = set()
    for stratum, members in strata.items():
        ranked = sorted(
            members,
            key=lambda pair: deterministic_rank(CROSSCHECK_SEED, pair.key),
        )
        for pair in ranked[: allocation[stratum]]:
            selected.add(pair.key)
    return selected, allocation


def deterministic_rank(seed: int, key: tuple[str, str, str]) -> str:
    return sha256_bytes(f"{seed}|{'|'.join(key)}".encode("utf-8"))


def load_crosscheck_selection(output_dir: Path) -> list[dict[str, str]]:
    path = output_dir / "crosscheck_manifest.csv"
    if not path.is_file():
        raise RuntimeError(f"missing frozen crosscheck selection: {path}")
    with path.open(newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))
    if not rows:
        raise RuntimeError("crosscheck selection is empty")
    keys = [(row["task_id"], row["artifact_id"], row["question_id"]) for row in rows]
    if len(keys) != len(set(keys)):
        raise RuntimeError("crosscheck selection contains duplicate keys")
    return rows


def validate_crosscheck_selection(
    *,
    output_dir: Path,
    pairs: Sequence[Pair],
    primary_by_key: Mapping[tuple[str, str, str], Mapping[str, Any]],
    selection: Sequence[Mapping[str, str]],
) -> None:
    """Fail closed unless the frozen selection exactly matches the protocol."""

    expected_rows, expected_sampling = derive_crosscheck_selection(pairs, primary_by_key)
    actual_rows = [dict(row) for row in selection]
    if actual_rows != expected_rows:
        raise RuntimeError("frozen crosscheck selection differs from recomputed protocol")
    rendered = render_csv(expected_rows, CROSSCHECK_SELECTION_FIELDS)
    expected_sampling["selection_hash"] = sha256_bytes(rendered.encode("utf-8"))
    sampling_path = output_dir / "crosscheck_sampling_manifest.json"
    if not sampling_path.is_file():
        raise RuntimeError(f"missing crosscheck sampling manifest: {sampling_path}")
    actual_sampling = json.loads(sampling_path.read_text(encoding="utf-8"))
    if actual_sampling != expected_sampling:
        raise RuntimeError("crosscheck sampling manifest differs from recomputed protocol")


def require_complete_crosscheck(
    output_dir: Path,
    selection: Sequence[Mapping[str, str]],
) -> None:
    artifacts = sorted(
        {(row["task_id"], row["artifact_id"]) for row in selection},
        key=artifact_sort_key,
    )
    missing = [
        key
        for key in artifacts
        if latest_successful_result(output_dir, "crosscheck", key[0], key[1]) is None
    ]
    if missing:
        raise RuntimeError(f"crosscheck pass incomplete for {len(missing)} artifacts: {missing}")


def load_pass_rows(
    output_dir: Path,
    grouped: Mapping[tuple[str, str], Sequence[Pair]],
    pass_id: str,
) -> dict[tuple[str, str, str], dict[str, Any]]:
    loaded = {}
    for artifact_key in ordered_artifact_keys(grouped):
        result = latest_successful_result(output_dir, pass_id, artifact_key[0], artifact_key[1])
        if result is None:
            continue
        expected_pairs = list(grouped[artifact_key])
        if pass_id == "crosscheck":
            requested_ids = set(result.provenance["question_ids"])
            expected_pairs = [
                pair for pair in expected_pairs if pair.source["question_id"] in requested_ids
            ]
        normalized = validate_payload(
            payload=result.payload,
            pairs=expected_pairs,
            artifact_root=ROOT / expected_pairs[0].source["artifact_path"],
        )
        if normalized != result.payload:
            raise RuntimeError(f"validated {pass_id} payload is not canonical for {artifact_key}")
        expected_config = PRIMARY_CONFIG if pass_id == "primary" else CROSSCHECK_CONFIG
        for field in ("model_id", "reasoning_effort", "pass_id"):
            if result.provenance.get(field) != expected_config[field]:
                raise RuntimeError(
                    f"validated {pass_id} provenance drift for {artifact_key}: {field}"
                )
        if result.provenance.get("prompt_version") != PROMPT_VERSION:
            raise RuntimeError(f"validated {pass_id} prompt version drift for {artifact_key}")
        for question_id, row in result.rows_by_question.items():
            key = (artifact_key[0], artifact_key[1], question_id)
            if key in loaded:
                raise RuntimeError(f"duplicate {pass_id} result row for {key}")
            loaded[key] = {
                "row": row,
                "provenance": result.provenance,
                "attempt_dir": result.attempt_dir,
            }
    return loaded


def stage_generation(
    *,
    output_dir: Path,
    contents: Mapping[str, str],
    validation: Mapping[str, Any],
    generation_kind: str = "release",
) -> tuple[str, Path, dict[str, Any]]:
    """Create and verify an immutable complete generation before publication."""

    content_hashes = {
        relative: sha256_bytes(content.encode("utf-8"))
        for relative, content in sorted(contents.items())
    }
    execution_manifest_path = output_dir / "execution_manifest.json"
    integrity_manifest_path = output_dir / "integrity_attestation_manifest.json"
    spec_path = output_dir / "SPEC.md"
    generation_basis = {
        "manifest_version": GENERATION_MANIFEST_VERSION,
        "generation_kind": generation_kind,
        "content_sha256": content_hashes,
        "transformation_script_sha256": sha256_file(Path(__file__).resolve()),
        "execution_manifest_sha256": sha256_file(execution_manifest_path),
        "integrity_attestation_manifest_sha256": sha256_file(integrity_manifest_path),
        "spec_version": SPEC_VERSION,
        "spec_sha256": sha256_file(spec_path),
        "validation": validation,
    }
    generation_id = sha256_bytes(
        json.dumps(
            generation_basis,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
    )
    generation_dir = output_dir / "generations" / generation_id
    generation_manifest = {
        **generation_basis,
        "generation_id": generation_id,
        "generation_path": display_path(generation_dir),
        "publication_contract": (
            "This immutable directory is authoritative for its generation kind. "
            "Root-level files are convenience aliases; the matching generation "
            "pointer is atomically replaced only after every alias is written."
        ),
    }

    if generation_dir.exists():
        manifest_path = generation_dir / "generation_manifest.json"
        if not manifest_path.is_file():
            raise RuntimeError(f"existing generation is incomplete: {generation_dir}")
        if json.loads(manifest_path.read_text(encoding="utf-8")) != generation_manifest:
            raise RuntimeError(f"existing generation manifest drifted: {generation_dir}")
        for relative, expected_hash in content_hashes.items():
            path = generation_dir / relative
            if not path.is_file() or sha256_file(path) != expected_hash:
                raise RuntimeError(f"existing generation content drifted: {path}")
        return generation_id, generation_dir, generation_manifest

    staging_parent = output_dir / ".generation-staging"
    staging_parent.mkdir(parents=True, exist_ok=True)
    staging_dir = staging_parent / f"{generation_id}-{os.getpid()}"
    staging_dir.mkdir()
    for relative, content in sorted(contents.items()):
        normalized = normalize_relative_path(relative)
        write_new_text(staging_dir / normalized, content)
    write_new_json(staging_dir / "generation_manifest.json", generation_manifest)
    for relative, expected_hash in content_hashes.items():
        path = staging_dir / relative
        if sha256_file(path) != expected_hash:
            raise RuntimeError(f"staged generation hash mismatch: {path}")
    if json.loads(
        (staging_dir / "generation_manifest.json").read_text(encoding="utf-8")
    ) != generation_manifest:
        raise RuntimeError("staged generation manifest failed round-trip validation")
    generation_dir.parent.mkdir(parents=True, exist_ok=True)
    staging_dir.replace(generation_dir)
    return generation_id, generation_dir, generation_manifest


def publish_generation(
    *,
    output_dir: Path,
    contents: Mapping[str, str],
    validation: Mapping[str, Any],
) -> str:
    """Publish aliases, then atomically switch the authoritative generation pointer."""

    generation_id, generation_dir, generation_manifest = stage_generation(
        output_dir=output_dir,
        contents=contents,
        validation=validation,
        generation_kind="release",
    )
    for relative, content in sorted(contents.items()):
        write_text(output_dir / relative, content)
    root_paths = [output_dir / relative for relative in sorted(contents)]
    for path in root_paths:
        expected = generation_manifest["content_sha256"][
            path.relative_to(output_dir).as_posix()
        ]
        if sha256_file(path) != expected:
            raise RuntimeError(f"published root alias hash mismatch: {path}")

    generation_manifest_path = generation_dir / "generation_manifest.json"
    output_manifest = {
        "manifest_version": GENERATION_MANIFEST_VERSION,
        "generation_id": generation_id,
        "generation_kind": "release",
        "current_generation_path": display_path(generation_dir),
        "generation_manifest_path": display_path(generation_manifest_path),
        "generation_manifest_sha256": sha256_file(generation_manifest_path),
        "transformation_script": display_path(Path(__file__).resolve()),
        "transformation_script_sha256": sha256_file(Path(__file__).resolve()),
        "transformation_command": (
            "python3 scripts/run_full_conformance_audit.py --phase finalize "
            "--engine collaboration"
        ),
        "row_grain": "one row per compile-pass implemented_gold artifact-question pair",
        "root_files_are_convenience_aliases": True,
        "output_paths": [display_path(path) for path in root_paths],
        "output_sha256": {display_path(path): sha256_file(path) for path in root_paths},
        "generation_output_sha256": {
            display_path(generation_dir / relative): digest
            for relative, digest in sorted(generation_manifest["content_sha256"].items())
        },
        "integrity_attestation_manifest_path": display_path(
            output_dir / "integrity_attestation_manifest.json"
        ),
        "integrity_attestation_manifest_sha256": sha256_file(
            output_dir / "integrity_attestation_manifest.json"
        ),
        "core_generation_pointer_path": display_path(
            output_dir / "CORE_GENERATION.json"
        ),
        "core_generation_pointer_sha256": sha256_file(
            output_dir / "CORE_GENERATION.json"
        ),
        "validation": validation,
    }
    output_manifest_path = output_dir / "output_manifest.json"
    write_json(output_manifest_path, output_manifest)
    pointer = {
        "manifest_version": GENERATION_MANIFEST_VERSION,
        "current_generation_id": generation_id,
        "current_generation_path": display_path(generation_dir),
        "generation_manifest_path": display_path(generation_manifest_path),
        "generation_manifest_sha256": sha256_file(generation_manifest_path),
        "root_output_manifest_path": display_path(output_manifest_path),
        "root_output_manifest_sha256": sha256_file(output_manifest_path),
    }
    # write_text uses same-directory temporary + os.replace, so this is the only
    # atomic switch consumers need to observe.
    write_json(output_dir / "CURRENT_GENERATION.json", pointer)
    return generation_id


def publish_core_generation(
    *,
    output_dir: Path,
    contents: Mapping[str, str],
    validation: Mapping[str, Any],
) -> str:
    """Atomically publish the stable canonical input consumed by sensitivity."""

    generation_id, generation_dir, generation_manifest = stage_generation(
        output_dir=output_dir,
        contents=contents,
        validation=validation,
        generation_kind="core_audit",
    )
    for relative, content in sorted(contents.items()):
        write_text(output_dir / relative, content)
    for relative, expected_hash in generation_manifest["content_sha256"].items():
        alias = output_dir / relative
        if sha256_file(alias) != expected_hash:
            raise RuntimeError(f"published core alias hash mismatch: {alias}")
    generation_manifest_path = generation_dir / "generation_manifest.json"
    pointer = {
        "manifest_version": GENERATION_MANIFEST_VERSION,
        "generation_kind": "core_audit",
        "current_generation_id": generation_id,
        "current_generation_path": display_path(generation_dir),
        "generation_manifest_path": display_path(generation_manifest_path),
        "generation_manifest_sha256": sha256_file(generation_manifest_path),
    }
    write_json(output_dir / "CORE_GENERATION.json", pointer)
    return generation_id


def finalize(
    output_dir: Path,
    pairs: Sequence[Pair],
    grouped: Mapping[tuple[str, str], Sequence[Pair]],
    selection: Sequence[Mapping[str, str]],
) -> None:
    verify_frozen_inputs(output_dir, grouped)
    integrity_manifest = verify_integrity_manifest(
        output_dir=output_dir,
        grouped=grouped,
    )
    primary = load_pass_rows(output_dir, grouped, "primary")
    crosscheck = load_pass_rows(output_dir, grouped, "crosscheck")
    expected_keys = {pair.key for pair in pairs}
    if set(primary) != expected_keys:
        raise RuntimeError(
            f"primary canonical row set mismatch: {len(primary)} != {len(expected_keys)}"
        )
    validate_crosscheck_selection(
        output_dir=output_dir,
        pairs=pairs,
        primary_by_key=primary,
        selection=selection,
    )
    selected_by_key = {
        (row["task_id"], row["artifact_id"], row["question_id"]): row for row in selection
    }
    if not set(selected_by_key).issubset(crosscheck):
        missing = sorted(set(selected_by_key) - set(crosscheck), key=pair_sort_key)
        raise RuntimeError(f"missing selected crosscheck rows: {missing}")
    extra_crosscheck = set(crosscheck) - set(selected_by_key)
    if extra_crosscheck:
        raise RuntimeError(f"crosscheck contains unselected rows: {sorted(extra_crosscheck)}")

    canonical_rows = []
    disagreement_rows = []
    excerpts = []
    pair_index_by_key = {pair.key: index for index, pair in enumerate(pairs, start=1)}
    for pair in pairs:
        key = pair.key
        first = primary[key]
        second = crosscheck.get(key)
        selection_row = selected_by_key.get(key)
        canonical = build_canonical_row(
            pair_index=pair_index_by_key[key],
            pair=pair,
            primary=first,
            crosscheck=second,
            selection=selection_row,
        )
        canonical_rows.append(canonical)
        append_excerpts(excerpts, pair, "primary", first)
        if second is not None:
            append_excerpts(excerpts, pair, "crosscheck", second)
        first_flag = first["row"]["verdict"] != "confirmed_gold"
        second_flag = second is not None and second["row"]["verdict"] != "confirmed_gold"
        if first_flag or second_flag:
            disagreement_rows.append(
                build_disagreement_row(pair, first, second, first_flag, second_flag)
            )

    task_rows = {
        "scratch_minidb": [row for row in canonical_rows if row["task_id"] == "scratch_minidb"],
        "scratch_nanoweb": [row for row in canonical_rows if row["task_id"] == "scratch_nanoweb"],
    }
    validation, regression_rows = validate_canonical_outputs(
        pairs=pairs,
        canonical_rows=canonical_rows,
        disagreements=disagreement_rows,
        selection=selection,
        frozen_inputs_verified=True,
    )
    validation = {
        **validation,
        "integrity_attestation_manifest_verified": True,
        "validated_attempt_attestations": integrity_manifest[
            "validated_attempt_count"
        ],
        "result_hash_source_counts": integrity_manifest[
            "result_hash_source_counts"
        ],
    }
    disagreement_csv = render_csv(disagreement_rows, DISAGREEMENT_FIELDS)
    disagreement_sha256 = sha256_bytes(disagreement_csv.encode("utf-8"))
    core_contents = {
        "minidb_adjudication.csv": render_csv(
            task_rows["scratch_minidb"],
            PRIMARY_FIELDS,
        ),
        "nanoweb_adjudication.csv": render_csv(
            task_rows["scratch_nanoweb"],
            PRIMARY_FIELDS,
        ),
        "disagreements.csv": disagreement_csv,
        "evidence_excerpts.jsonl": render_jsonl(excerpts),
        "regression_check.csv": render_csv(regression_rows, REGRESSION_FIELDS),
        "validation_manifest.json": render_json(validation),
    }
    core_generation_id = publish_core_generation(
        output_dir=output_dir,
        contents=core_contents,
        validation=validation,
    )
    report_text = render_report(
        output_dir=output_dir,
        canonical_rows=canonical_rows,
        disagreements=disagreement_rows,
        selection=selection,
        validation=validation,
        regression_rows=regression_rows,
        disagreement_sha256=disagreement_sha256,
    )
    sensitivity_paths = validated_sensitivity_bundle(
        output_dir,
        expected_disagreement_sha256=disagreement_sha256,
        expected_author_flag_count=len(disagreement_rows),
    )
    contents = {
        **core_contents,
        "report.md": report_text,
    }
    if sensitivity_paths is not None:
        for path in sensitivity_paths:
            relative = path.relative_to(output_dir).as_posix()
            contents[relative] = path.read_text(encoding="utf-8")
    generation_id = publish_generation(
        output_dir=output_dir,
        contents=contents,
        validation=validation,
    )
    print(
        f"finalized {len(canonical_rows)} pairs; author flags={len(disagreement_rows)}; "
        f"core_generation={core_generation_id}; release_generation={generation_id}",
        flush=True,
    )


def build_canonical_row(
    *,
    pair_index: int,
    pair: Pair,
    primary: Mapping[str, Any],
    crosscheck: Mapping[str, Any] | None,
    selection: Mapping[str, str] | None,
) -> dict[str, Any]:
    source = pair.source
    first_row = primary["row"]
    first_prov = primary["provenance"]
    second_row = crosscheck["row"] if crosscheck else None
    second_prov = crosscheck["provenance"] if crosscheck else None
    first_label = mapped_label(first_row["verdict"])
    first_agrees = first_row["verdict"] == "confirmed_gold"
    second_agrees = (
        second_row["verdict"] == first_row["verdict"] if second_row is not None else None
    )
    first_flag = not first_agrees
    second_flag = second_row is not None and second_row["verdict"] != "confirmed_gold"
    if second_row is None:
        review_status = "primary_only"
    elif second_agrees:
        review_status = "crosscheck_agrees"
    else:
        review_status = "crosscheck_conflict"
    return {
        "sample_index": pair_index,
        "task_id": source["task_id"],
        "artifact_id": source["artifact_id"],
        "question_id": source["question_id"],
        "consensus_label": source["final_label"],
        "source_audit_label": first_label,
        "agrees_with_consensus": bool_text(first_agrees),
        "verdict": first_row["verdict"],
        "actual_option": first_row["actual_option"],
        "confidence": first_row["confidence"],
        "runtime_reachability_checked": "true",
        "evidence_files": join_values(first_row["evidence_files"]),
        "evidence_symbols": join_values(first_row["evidence_symbols"]),
        "evidence_spans": format_spans(first_row["evidence_spans"]),
        "rationale": first_row["rationale"],
        "adjudicator": (
            f"{first_prov['model_id']}:{first_prov['reasoning_effort']}:"
            f"{first_prov['prompt_version']}"
        ),
        "execution_engine": first_prov["execution_engine"],
        "model_id": first_prov["model_id"],
        "reasoning_effort": first_prov["reasoning_effort"],
        "prompt_version": first_prov["prompt_version"],
        "prompt_template_sha256": first_prov["prompt_template_sha256"],
        "rendered_prompt_sha256": first_prov["rendered_prompt_sha256"],
        "codex_cli_version": first_prov["codex_cli_version"],
        "session_id": first_prov["session_id"],
        "session_id_source": first_prov["session_id_source"],
        "model_id_source": first_prov["model_id_source"],
        "session_result_sha256": first_prov["selected_result_sha256"],
        "session_result_hash_source": first_prov["result_hash_source"],
        "integrity_attestation_sha256": first_prov[
            "integrity_attestation_sha256"
        ],
        "artifact_path": source["artifact_path"],
        "builder_id": source["builder_id"],
        "builder_family": source["builder_family"],
        "builder_effort": source["builder_effort"],
        "build_trial_id": source["build_trial_id"],
        "question_type": source["question_type"],
        "gold_canonical_option": source["gold_canonical_option"],
        "source_requirement_id": source["source_requirement_id"],
        "source_intention_id": source["source_intention_id"],
        "current_label_source": source["label_source"],
        "crosscheck_selected": bool_text(second_row is not None),
        "crosscheck_selection_reason": selection["selection_reason"] if selection else "",
        "crosscheck_verdict": second_row["verdict"] if second_row else "",
        "crosscheck_source_audit_label": (
            mapped_label(second_row["verdict"]) if second_row else ""
        ),
        "crosscheck_actual_option": second_row["actual_option"] if second_row else "",
        "crosscheck_confidence": second_row["confidence"] if second_row else "",
        "crosscheck_runtime_reachability_checked": "true" if second_row else "",
        "crosscheck_evidence_files": (
            join_values(second_row["evidence_files"]) if second_row else ""
        ),
        "crosscheck_evidence_symbols": (
            join_values(second_row["evidence_symbols"]) if second_row else ""
        ),
        "crosscheck_evidence_spans": (
            format_spans(second_row["evidence_spans"]) if second_row else ""
        ),
        "crosscheck_rationale": second_row["rationale"] if second_row else "",
        "crosscheck_execution_engine": (second_prov["execution_engine"] if second_prov else ""),
        "crosscheck_model_id": second_prov["model_id"] if second_prov else "",
        "crosscheck_reasoning_effort": (second_prov["reasoning_effort"] if second_prov else ""),
        "crosscheck_prompt_version": (second_prov["prompt_version"] if second_prov else ""),
        "crosscheck_prompt_template_sha256": (
            second_prov["prompt_template_sha256"] if second_prov else ""
        ),
        "crosscheck_rendered_prompt_sha256": (
            second_prov["rendered_prompt_sha256"] if second_prov else ""
        ),
        "crosscheck_codex_cli_version": (second_prov["codex_cli_version"] if second_prov else ""),
        "crosscheck_session_id": second_prov["session_id"] if second_prov else "",
        "crosscheck_session_id_source": (second_prov["session_id_source"] if second_prov else ""),
        "crosscheck_model_id_source": (second_prov["model_id_source"] if second_prov else ""),
        "crosscheck_session_result_sha256": (
            second_prov["selected_result_sha256"] if second_prov else ""
        ),
        "crosscheck_session_result_hash_source": (
            second_prov["result_hash_source"] if second_prov else ""
        ),
        "crosscheck_integrity_attestation_sha256": (
            second_prov["integrity_attestation_sha256"] if second_prov else ""
        ),
        "crosscheck_agrees_with_primary": (
            bool_text(second_agrees) if second_agrees is not None else ""
        ),
        "review_status": review_status,
        "flagged_for_author": bool_text(first_flag or second_flag),
    }


def build_disagreement_row(
    pair: Pair,
    primary: Mapping[str, Any],
    crosscheck: Mapping[str, Any] | None,
    first_flag: bool,
    second_flag: bool,
) -> dict[str, Any]:
    first = primary["row"]
    first_prov = primary["provenance"]
    second = crosscheck["row"] if crosscheck else None
    second_prov = crosscheck["provenance"] if crosscheck else None
    flag_verdicts = [
        row["verdict"]
        for row in (first, second)
        if row is not None and row["verdict"] != "confirmed_gold"
    ]
    if first_flag and second_flag:
        basis = "both_passes"
    elif first_flag:
        basis = "primary_pass"
    else:
        basis = "crosscheck_pass"
    unique_flags = set(flag_verdicts)
    proposed = "implemented_non_gold" if unique_flags == {"non_gold"} else "absent_or_ambiguous"
    return {
        "task_id": pair.source["task_id"],
        "artifact_id": pair.source["artifact_id"],
        "question_id": pair.source["question_id"],
        "builder_id": pair.source["builder_id"],
        "builder_family": pair.source["builder_family"],
        "builder_effort": pair.source["builder_effort"],
        "consensus_label": pair.source["final_label"],
        "current_label_source": pair.source["label_source"],
        "primary_verdict": first["verdict"],
        "primary_source_audit_label": mapped_label(first["verdict"]),
        "primary_confidence": first["confidence"],
        "primary_evidence_files": join_values(first["evidence_files"]),
        "primary_evidence_symbols": join_values(first["evidence_symbols"]),
        "primary_evidence_spans": format_spans(first["evidence_spans"]),
        "primary_rationale": first["rationale"],
        "primary_execution_engine": first_prov["execution_engine"],
        "primary_model_id": first_prov["model_id"],
        "primary_reasoning_effort": first_prov["reasoning_effort"],
        "primary_prompt_version": first_prov["prompt_version"],
        "primary_prompt_template_sha256": first_prov["prompt_template_sha256"],
        "primary_rendered_prompt_sha256": first_prov["rendered_prompt_sha256"],
        "primary_session_id": first_prov["session_id"],
        "primary_session_id_source": first_prov["session_id_source"],
        "primary_model_id_source": first_prov["model_id_source"],
        "primary_session_result_sha256": first_prov["selected_result_sha256"],
        "primary_session_result_hash_source": first_prov["result_hash_source"],
        "primary_integrity_attestation_sha256": first_prov[
            "integrity_attestation_sha256"
        ],
        "crosscheck_verdict": second["verdict"] if second else "",
        "crosscheck_source_audit_label": (mapped_label(second["verdict"]) if second else ""),
        "crosscheck_confidence": second["confidence"] if second else "",
        "crosscheck_evidence_files": (join_values(second["evidence_files"]) if second else ""),
        "crosscheck_evidence_symbols": (join_values(second["evidence_symbols"]) if second else ""),
        "crosscheck_evidence_spans": (format_spans(second["evidence_spans"]) if second else ""),
        "crosscheck_rationale": second["rationale"] if second else "",
        "crosscheck_execution_engine": (second_prov["execution_engine"] if second_prov else ""),
        "crosscheck_model_id": second_prov["model_id"] if second_prov else "",
        "crosscheck_reasoning_effort": (second_prov["reasoning_effort"] if second_prov else ""),
        "crosscheck_prompt_version": (second_prov["prompt_version"] if second_prov else ""),
        "crosscheck_prompt_template_sha256": (
            second_prov["prompt_template_sha256"] if second_prov else ""
        ),
        "crosscheck_rendered_prompt_sha256": (
            second_prov["rendered_prompt_sha256"] if second_prov else ""
        ),
        "crosscheck_session_id": second_prov["session_id"] if second_prov else "",
        "crosscheck_session_id_source": (second_prov["session_id_source"] if second_prov else ""),
        "crosscheck_model_id_source": (second_prov["model_id_source"] if second_prov else ""),
        "crosscheck_session_result_sha256": (
            second_prov["selected_result_sha256"] if second_prov else ""
        ),
        "crosscheck_session_result_hash_source": (
            second_prov["result_hash_source"] if second_prov else ""
        ),
        "crosscheck_integrity_attestation_sha256": (
            second_prov["integrity_attestation_sha256"] if second_prov else ""
        ),
        "crosscheck_agrees_with_primary": (
            bool_text(second["verdict"] == first["verdict"]) if second else ""
        ),
        "disagreement_basis": basis,
        "flag_verdicts": "|".join(flag_verdicts),
        "proposed_sensitivity_label": proposed,
        "author_adjudication_status": "pending",
    }


def append_excerpts(
    excerpts: list[dict[str, Any]],
    pair: Pair,
    pass_id: str,
    record: Mapping[str, Any],
) -> None:
    artifact_root = ROOT / pair.source["artifact_path"]
    row = record["row"]
    for span_index, span in enumerate(row["evidence_spans"], start=1):
        path = resolve_evidence_file(artifact_root, span["file"])
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        selected = lines[span["start_line"] - 1 : span["end_line"]]
        excerpts.append(
            {
                "task_id": pair.source["task_id"],
                "artifact_id": pair.source["artifact_id"],
                "question_id": pair.source["question_id"],
                "pass_id": pass_id,
                "model_id": record["provenance"]["model_id"],
                "prompt_version": record["provenance"]["prompt_version"],
                "session_id": record["provenance"]["session_id"],
                "span_index": span_index,
                "file": span["file"],
                "start_line": span["start_line"],
                "end_line": span["end_line"],
                "file_sha256": sha256_file(path),
                "excerpt": "\n".join(
                    f"{number:>6}\t{text}"
                    for number, text in enumerate(selected, start=span["start_line"])
                ),
            }
        )


def validate_canonical_outputs(
    *,
    pairs: Sequence[Pair],
    canonical_rows: Sequence[Mapping[str, Any]],
    disagreements: Sequence[Mapping[str, Any]],
    selection: Sequence[Mapping[str, str]],
    frozen_inputs_verified: bool,
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    if not frozen_inputs_verified:
        raise RuntimeError("canonical validation requires a successful frozen-input check")
    if len(canonical_rows) != EXPECTED_PAIRS:
        raise RuntimeError("canonical adjudication row count is not 592")
    keys = [(row["task_id"], row["artifact_id"], row["question_id"]) for row in canonical_rows]
    if len(keys) != len(set(keys)):
        raise RuntimeError("canonical adjudication contains duplicate keys")
    if set(keys) != {pair.key for pair in pairs}:
        raise RuntimeError("canonical adjudication key set differs from population")
    if any(row["runtime_reachability_checked"] != "true" for row in canonical_rows):
        raise RuntimeError("canonical row lacks primary reachability check")
    if any(
        not row["model_id"]
        or not row["prompt_version"]
        or not row["evidence_spans"]
        or not row["session_result_hash_source"]
        or not row["integrity_attestation_sha256"]
        for row in canonical_rows
    ):
        raise RuntimeError("canonical row lacks model/prompt/span/integrity provenance")
    allowed_hash_sources = {
        "completion_record_and_parent_verified",
        "post_hoc_parent_attestation",
    }
    if any(
        row["session_result_hash_source"] not in allowed_hash_sources
        for row in canonical_rows
    ):
        raise RuntimeError("canonical row has invalid primary result-hash source")
    if any(
        re.fullmatch(r"[0-9a-f]{64}", row["integrity_attestation_sha256"]) is None
        for row in canonical_rows
    ):
        raise RuntimeError("canonical row has invalid primary attestation hash")
    if any(
        row["crosscheck_selected"] == "true"
        and (
            row["crosscheck_session_result_hash_source"] not in allowed_hash_sources
            or re.fullmatch(
                r"[0-9a-f]{64}",
                row["crosscheck_integrity_attestation_sha256"],
            )
            is None
        )
        for row in canonical_rows
    ):
        raise RuntimeError("selected crosscheck row lacks integrity provenance")
    selected_count = sum(row["crosscheck_selected"] == "true" for row in canonical_rows)
    if selected_count != len(selection):
        raise RuntimeError("canonical crosscheck count differs from selection")
    disagreement_keys = {
        (row["task_id"], row["artifact_id"], row["question_id"]) for row in disagreements
    }
    regression_rows = []
    for key, expected_label in sorted(
        KNOWN_REGRESSIONS.items(), key=lambda item: pair_sort_key(item[0])
    ):
        row = next(
            candidate
            for candidate in canonical_rows
            if (
                candidate["task_id"],
                candidate["artifact_id"],
                candidate["question_id"],
            )
            == key
        )
        primary_flag = row["verdict"] != "confirmed_gold"
        crosscheck_flag = (
            row["crosscheck_verdict"] and row["crosscheck_verdict"] != "confirmed_gold"
        )
        primary_mapped_label = mapped_label(row["verdict"])
        crosscheck_mapped_label = (
            mapped_label(row["crosscheck_verdict"]) if row["crosscheck_verdict"] else ""
        )
        primary_reproduced = verdict_reproduces_label(row["verdict"], expected_label)
        crosscheck_reproduced = bool(row["crosscheck_verdict"]) and verdict_reproduces_label(
            row["crosscheck_verdict"], expected_label
        )
        reproduced = primary_reproduced or crosscheck_reproduced
        if reproduced != (key in disagreement_keys):
            raise RuntimeError(
                f"known regression disagreement union mismatch for {key}: "
                f"expected_label={expected_label}"
            )
        regression_rows.append(
            {
                "task_id": key[0],
                "artifact_id": key[1],
                "question_id": key[2],
                "expected_mapped_label": expected_label,
                "primary_verdict": row["verdict"],
                "primary_mapped_label": primary_mapped_label,
                "crosscheck_verdict": row["crosscheck_verdict"],
                "crosscheck_mapped_label": crosscheck_mapped_label,
                "primary_flag": primary_flag,
                "crosscheck_flag": bool(crosscheck_flag),
                "primary_reproduced_expected_label": primary_reproduced,
                "crosscheck_reproduced_expected_label": crosscheck_reproduced,
                "reproduced_by_either_pass": reproduced,
                "reproduced_by_both_passes": bool(primary_reproduced and crosscheck_reproduced),
            }
        )
    reproduced = sum(row["reproduced_by_either_pass"] for row in regression_rows)
    if reproduced != len(KNOWN_REGRESSIONS):
        missing = [
            (row["task_id"], row["artifact_id"], row["question_id"])
            for row in regression_rows
            if not row["reproduced_by_either_pass"]
        ]
        raise RuntimeError(f"known-error regression check failed: {missing}")
    validation = {
        "population_pairs": len(canonical_rows),
        "population_artifacts": len(
            {(row["task_id"], row["artifact_id"]) for row in canonical_rows}
        ),
        "primary_rows_with_reachability": sum(
            row["runtime_reachability_checked"] == "true" for row in canonical_rows
        ),
        "primary_rows_with_evidence_spans": sum(
            bool(row["evidence_spans"]) for row in canonical_rows
        ),
        "primary_rows_with_model_and_prompt": sum(
            bool(row["model_id"] and row["prompt_version"]) for row in canonical_rows
        ),
        "primary_rows_with_integrity_attestation": sum(
            bool(row["integrity_attestation_sha256"]) for row in canonical_rows
        ),
        "primary_result_hash_source_counts": dict(
            Counter(row["session_result_hash_source"] for row in canonical_rows)
        ),
        "crosscheck_selected_pairs": selected_count,
        "crosscheck_rows_with_reachability": sum(
            row["crosscheck_runtime_reachability_checked"] == "true" for row in canonical_rows
        ),
        "crosscheck_rows_with_integrity_attestation": sum(
            bool(row["crosscheck_integrity_attestation_sha256"])
            for row in canonical_rows
        ),
        "crosscheck_result_hash_source_counts": dict(
            Counter(
                row["crosscheck_session_result_hash_source"]
                for row in canonical_rows
                if row["crosscheck_selected"] == "true"
            )
        ),
        "author_flag_count": len(disagreements),
        "known_regressions_reproduced_by_either_pass": reproduced,
        "known_regressions_reproduced_by_both_passes": sum(
            row["reproduced_by_both_passes"] for row in regression_rows
        ),
        "runs_release_tree_verified_unchanged": frozen_inputs_verified,
    }
    return validation, regression_rows


def render_report(
    *,
    output_dir: Path,
    canonical_rows: Sequence[Mapping[str, Any]],
    disagreements: Sequence[Mapping[str, Any]],
    selection: Sequence[Mapping[str, str]],
    validation: Mapping[str, Any],
    regression_rows: Sequence[Mapping[str, Any]],
    disagreement_sha256: str,
) -> str:
    first_counts = Counter(row["verdict"] for row in canonical_rows)
    union_flag_keys = {
        (row["task_id"], row["artifact_id"], row["question_id"]) for row in disagreements
    }
    crosschecked = [row for row in canonical_rows if row["crosscheck_selected"] == "true"]
    crosscheck_agreement = sum(
        row["crosscheck_agrees_with_primary"] == "true" for row in crosschecked
    )
    task_stats = []
    for task_id in TASK_ORDER:
        rows = [row for row in canonical_rows if row["task_id"] == task_id]
        flags = sum(
            (row["task_id"], row["artifact_id"], row["question_id"]) in union_flag_keys
            for row in rows
        )
        task_stats.append((task_id, rows, flags))
    family_stats = []
    for task_id in TASK_ORDER:
        for family in sorted({row["builder_family"] for row in canonical_rows}):
            rows = [
                row
                for row in canonical_rows
                if row["task_id"] == task_id and row["builder_family"] == family
            ]
            if not rows:
                continue
            flags = sum(
                (row["task_id"], row["artifact_id"], row["question_id"]) in union_flag_keys
                for row in rows
            )
            family_stats.append((task_id, family, rows, flags))

    lines = [
        "# Full-coverage source-trace conformance audit",
        "",
        "## Outcome",
        "",
        f"The primary pass covered all **{len(canonical_rows)} / {EXPECTED_PAIRS}** "
        f"compile-passing pairs currently labeled `implemented_gold`, across "
        f"**{validation['population_artifacts']} / {EXPECTED_ARTIFACTS}** artifacts. "
        f"Every primary row has a live-path check, concrete file/symbol evidence, "
        f"validated line spans, and exact model/prompt/session provenance. Every "
        f"selected result is bound by an append-only integrity attestation.",
        "",
        "These are independent frontier-model source reviews, not human annotations. "
        "Every flag from either pass remains pending author adjudication; the audit "
        "does not modify `runs_release/` or self-resolve review conflicts.",
        "",
        "## Models and independence",
        "",
        "| pass | model | effort | prompt version | unit |",
        "| --- | --- | --- | --- | --- |",
        f"| primary | `{PRIMARY_CONFIG['model_id']}` | "
        f"`{PRIMARY_CONFIG['reasoning_effort']}` | `{PROMPT_VERSION}` | "
        "one fresh session per artifact |",
        f"| cross-check | `{CROSSCHECK_CONFIG['model_id']}` | "
        f"`{CROSSCHECK_CONFIG['reasoning_effort']}` | `{PROMPT_VERSION}` | "
        "one fresh session per selected artifact |",
        "",
        "Primary and cross-check prompts contain the canonical question/options, gold "
        "option, spec trace, and artifact path, but no current label, finder vote, "
        "prior verdict, prior rationale, or cross-check selection reason. Labels are "
        "joined only after source judgment.",
        "",
        "## Coverage and primary verdicts",
        "",
        "| verdict | pairs | rate |",
        "| --- | ---: | ---: |",
    ]
    for verdict in ("confirmed_gold", "non_gold", "absent", "ambiguous"):
        count = first_counts[verdict]
        lines.append(f"| {verdict} | {count} | {pct(count, len(canonical_rows))} |")
    lines.extend(
        [
            "",
            "| task | pairs | primary confirmed | primary agreement | "
            "flagged by either pass | union agreement |",
            "| --- | ---: | ---: | ---: | ---: | ---: |",
        ]
    )
    for task_id, rows, flags in task_stats:
        confirmed = sum(row["verdict"] == "confirmed_gold" for row in rows)
        lines.append(
            f"| {task_id} | {len(rows)} | {confirmed} | "
            f"{pct(confirmed, len(rows))} | {flags} | "
            f"{pct(len(rows) - flags, len(rows))} |"
        )
    lines.extend(
        [
            "",
            "## Agreement by task and builder family",
            "",
            "| task | builder family | pairs | primary confirmed | "
            "primary agreement | flagged by either pass | union agreement |",
            "| --- | --- | ---: | ---: | ---: | ---: | ---: |",
        ]
    )
    for task_id, family, rows, flags in family_stats:
        confirmed = sum(row["verdict"] == "confirmed_gold" for row in rows)
        lines.append(
            f"| {task_id} | {family} | {len(rows)} | {confirmed} | "
            f"{pct(confirmed, len(rows))} | {flags} | "
            f"{pct(len(rows) - flags, len(rows))} |"
        )
    lines.extend(
        [
            "",
            "## Independent cross-check",
            "",
            f"The second pass reviewed **{len(selection)}** pairs: every primary flag, "
            f"an exact 10% task × builder-family stratified sample of primary "
            f"`confirmed_gold` pairs (rounded up once at the population level), and "
            f"the frozen nine-key regression set. The fresh `{CROSSCHECK_CONFIG['model_id']}` "
            "sessions received neither primary decisions nor selection reasons.",
            "",
            "| selected pairs | same verdict as primary | verdict agreement |",
            "| ---: | ---: | ---: |",
            f"| {len(crosschecked)} | {crosscheck_agreement} | "
            f"{pct(crosscheck_agreement, len(crosschecked))} |",
            "",
            f"`disagreements.csv` contains **{len(disagreements)}** unique pairs flagged "
            "by at least one pass. A cross-pass conflict remains a flag and is marked "
            "`author_adjudication_status=pending`.",
            "",
            "## Known-error regression check",
            "",
            "| task | artifact | question | expected mapped label | primary | "
            "cross-check | exact either | exact both |",
            "| --- | --- | --- | --- | --- | --- | ---: | ---: |",
        ]
    )
    for row in regression_rows:
        lines.append(
            f"| {row['task_id']} | `{row['artifact_id']}` | {row['question_id']} | "
            f"{row['expected_mapped_label']} | {row['primary_verdict']} | "
            f"{row['crosscheck_verdict']} | "
            f"{row['reproduced_by_either_pass']} | "
            f"{row['reproduced_by_both_passes']} |"
        )
    lines.extend(
        [
            "",
            f"All **{validation['known_regressions_reproduced_by_either_pass']} / 9** "
            "known label errors were reproduced at the expected mapped-label level. "
            "The table exposes whether both independent passes reproduced each case "
            "rather than treating any non-gold flag as sufficient.",
            "",
            "## Label-sensitivity analysis",
            "",
        ]
    )
    sensitivity = sensitivity_summary(
        output_dir,
        expected_disagreement_sha256=disagreement_sha256,
        expected_author_flag_count=len(disagreements),
    )
    if sensitivity is None:
        lines.extend(
            [
                "Pending. Run `scripts/analyze_rebuttal_label_sensitivity.py` with "
                "`--full-audit-disagreements "
                "rebuttal/full_conformance_audit/disagreements.csv --output-dir "
                "rebuttal/full_conformance_audit/label_sensitivity`, then rerun this "
                "script's `--phase finalize` to embed the results.",
            ]
        )
    else:
        lines.extend(sensitivity)
    lines.extend(
        [
            "",
            "## Traceability and files",
            "",
            "- `minidb_adjudication.csv` and `nanoweb_adjudication.csv`: one canonical "
            "row per pair, mirroring the spot-audit fields and adding spans plus "
            "primary/cross-check model and prompt provenance.",
            "- `disagreements.csv`: union of all pairs flagged by either pass; no "
            "released label is changed.",
            "- `evidence_excerpts.jsonl`: frozen line-numbered text for every cited "
            "span with source-file hashes.",
            "- `raw/`: append-only request, prompt, assignment, result, completion, "
            "and integrity-attestation records per session attempt.",
            "- `integrity_attestation_manifest.json`: closed ledger of all 82 "
            "successful attempts, explicitly distinguishing completion-time from "
            "post-hoc result hashes.",
            "- `CORE_GENERATION.json`: stable canonical-data generation consumed by "
            "the sensitivity analysis; it excludes the report and sensitivity outputs.",
            "- `generations/<content-hash>/` plus `CURRENT_GENERATION.json`: the "
            "immutable release bundle and its atomic publication pointer. Root-level "
            "CSV/report files are convenience aliases.",
            "- `execution_manifest.json`, `crosscheck_sampling_manifest.json`, "
            "`validation_manifest.json`, and `output_manifest.json`: commands, hashes, "
            "row counts, selection rules, and validation results.",
            "",
            "## Limitations",
            "",
            "This is full coverage of the scoring pairs, but still model-assisted "
            "static source tracing. It does not estimate an error rate from the older "
            "24-pair spot sample, and it does not elevate unresolved model flags to "
            "human-adjudicated ground truth. Dynamic dispatch or runtime configuration "
            "can still be misread; file/symbol/span records are provided so the author "
            "can adjudicate every flagged pair directly. Prompt blinding is procedural: "
            "fresh no-history agents were instructed not to inspect labels or prior "
            "results, but the shared workspace remained technically readable and the "
            "collaboration runtime exposes no filesystem-access trace. Model ids are "
            "the exact parent dispatch requests, not separately attested runtime tokens. "
            "For 43 historical sessions, the result hash was appended and verified "
            "after completion rather than recorded at completion time; this provenance "
            "is explicit in every affected row.",
            "",
        ]
    )
    return "\n".join(lines)


def validated_sensitivity_bundle(
    output_dir: Path,
    *,
    expected_disagreement_sha256: str,
    expected_author_flag_count: int,
) -> list[Path] | None:
    sensitivity_dir = output_dir / "label_sensitivity"
    rb_path = sensitivity_dir / "rb_rankings.csv"
    coverage_path = sensitivity_dir / "implementation_coverage.csv"
    manifest_path = sensitivity_dir / "manifest.json"
    if not rb_path.is_file() or not coverage_path.is_file() or not manifest_path.is_file():
        return None
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("manifest_version") != SENSITIVITY_MANIFEST_VERSION:
        return None
    if manifest.get("analysis_id") != "rebuttal_label_sensitivity":
        raise RuntimeError("sensitivity manifest has an invalid analysis id")
    analysis_script = ROOT / "scripts" / "analyze_rebuttal_label_sensitivity.py"
    if manifest.get("analysis_script") != display_path(analysis_script):
        raise RuntimeError("sensitivity manifest points to a different analysis script")
    if manifest.get("analysis_script_sha256") != sha256_file(analysis_script):
        return None

    disagreement_path = output_dir / "disagreements.csv"
    expected_inputs = {
        "final_labels": LABELS_PATH,
        "find_runs": RUNS_RELEASE / "tables" / "find_runs.csv",
        "question_answers": RUNS_RELEASE / "tables" / "question_answers.csv",
        "low_prior_manifest": (
            RUNS_RELEASE / "reports" / "eandd_final" / "low_prior" / "manifest.json"
        ),
        "published_builder_scores": (
            RUNS_RELEASE
            / "reports"
            / "artifact_conformance_consensus"
            / "impact"
            / "conditioned_builder_scores.csv"
        ),
        "spot_audit_minidb": (
            ROOT / "rebuttal" / "conformance_spot_audit" / "minidb_adjudication.csv"
        ),
        "spot_audit_nanoweb": (
            ROOT / "rebuttal" / "conformance_spot_audit" / "nanoweb_adjudication.csv"
        ),
        "spot_audit_sample_manifest": (
            ROOT / "rebuttal" / "conformance_spot_audit" / "sample_manifest.csv"
        ),
        "full_audit_disagreements": disagreement_path,
        "full_audit_minidb_adjudication": output_dir / "minidb_adjudication.csv",
        "full_audit_nanoweb_adjudication": output_dir / "nanoweb_adjudication.csv",
        "full_audit_validation_manifest": output_dir / "validation_manifest.json",
        "full_audit_core_generation": output_dir / "CORE_GENERATION.json",
    }
    if set(expected_inputs) != SENSITIVITY_INPUT_KEYS:
        raise RuntimeError("internal sensitivity input contract drift")
    input_paths = manifest.get("input_paths")
    input_hashes = manifest.get("input_sha256")
    if not isinstance(input_paths, dict) or set(input_paths) != SENSITIVITY_INPUT_KEYS:
        raise RuntimeError("sensitivity manifest input-path set is incomplete")
    if not isinstance(input_hashes, dict) or set(input_hashes) != SENSITIVITY_INPUT_KEYS:
        raise RuntimeError("sensitivity manifest input-hash set is incomplete")
    for name, path in expected_inputs.items():
        if input_paths.get(name) != display_path(path):
            raise RuntimeError(f"sensitivity manifest input path drift: {name}")
        if not path.is_file():
            raise RuntimeError(f"sensitivity semantic input is missing: {path}")
        if input_hashes.get(name) != sha256_file(path):
            return None

    recorded_disagreement_sha256 = manifest.get("input_sha256", {}).get(
        "full_audit_disagreements"
    )
    if recorded_disagreement_sha256 != expected_disagreement_sha256:
        return None
    if manifest.get("full_audit_flag_count") != expected_author_flag_count:
        raise RuntimeError("sensitivity manifest flag count differs from finalized audit")

    core_pointer = json.loads(
        (output_dir / "CORE_GENERATION.json").read_text(encoding="utf-8")
    )
    bundle_validation = manifest.get("full_audit_bundle_validation")
    if not isinstance(bundle_validation, dict) or not bundle_validation.get(
        "current_core_generation_verified"
    ):
        raise RuntimeError("sensitivity manifest lacks verified core-generation provenance")
    if bundle_validation.get("core_generation_id_at_analysis") != core_pointer.get(
        "current_generation_id"
    ):
        return None

    expected_output_names = {
        "corrections.csv",
        "label_accuracy.csv",
        "implementation_coverage.csv",
        "implementation_coverage_changes.csv",
        "conditioned_accuracy.csv",
        "rb_rankings.csv",
        "rb_rank_changes.csv",
        "perfect_run_summary.csv",
        "perfect_cell_summary.csv",
        "run_metric_changes.csv",
        "perfect_cell_changes.csv",
        "report.md",
    }
    paths = []
    for raw_path in manifest.get("output_paths", []):
        path = Path(raw_path)
        resolved = path.resolve() if path.is_absolute() else (ROOT / path).resolve()
        if not resolved.is_relative_to(sensitivity_dir.resolve()):
            raise RuntimeError("sensitivity output path escapes sensitivity directory")
        if not resolved.is_file():
            raise RuntimeError(f"sensitivity output is missing: {resolved}")
        if manifest.get("output_sha256", {}).get(display_path(resolved)) != sha256_file(
            resolved
        ):
            raise RuntimeError(f"sensitivity output hash mismatch: {resolved}")
        paths.append(resolved)
    if {path.name for path in paths} != expected_output_names:
        raise RuntimeError("sensitivity manifest output set is incomplete or unexpected")
    if rb_path not in paths or coverage_path not in paths:
        raise RuntimeError("sensitivity manifest omits required ranking/coverage outputs")
    paths.append(manifest_path)
    return list(dict.fromkeys(paths))


def sensitivity_summary(
    output_dir: Path,
    *,
    expected_disagreement_sha256: str,
    expected_author_flag_count: int,
) -> list[str] | None:
    sensitivity_paths = validated_sensitivity_bundle(
        output_dir,
        expected_disagreement_sha256=expected_disagreement_sha256,
        expected_author_flag_count=expected_author_flag_count,
    )
    if sensitivity_paths is None:
        return None
    sensitivity_dir = output_dir / "label_sensitivity"
    rb_path = sensitivity_dir / "rb_rankings.csv"
    coverage_path = sensitivity_dir / "implementation_coverage.csv"
    rb_rows = read_csv(rb_path)
    coverage_rows = read_csv(coverage_path)
    scenario = "full_audit_flags_corrected"
    baseline_order = panel_builder_order(rb_rows, "baseline")
    full_order = panel_builder_order(rb_rows, scenario)
    coverage_ranges = {}
    for scenario_id in ("baseline", scenario):
        values = [
            float(row["implementation_coverage"])
            for row in coverage_rows
            if row["scenario"] == scenario_id
            and row["scope_id"] == "artifact_present_all"
            and row["grain"] == "builder"
        ]
        if values:
            coverage_ranges[scenario_id] = (min(values), max(values))
    if set(coverage_ranges) != {"baseline", scenario}:
        raise RuntimeError("sensitivity outputs lack full-audit coverage scenario")
    lines = [
        "For sensitivity only, every unresolved full-audit flag is overlaid as a "
        "relabeling away from `implemented_gold`; this does not adjudicate the flags.",
        "",
        "| panel | baseline builder order | full-audit order | changed |",
        "| --- | --- | --- | ---: |",
    ]
    for panel_id in ("high_effort", "low_effort"):
        before = baseline_order.get(panel_id, [])
        after = full_order.get(panel_id, [])
        lines.append(
            f"| {panel_id} | {' > '.join(before)} | {' > '.join(after)} | "
            f"{bool_text(before != after)} |"
        )
    before_range = coverage_ranges["baseline"]
    after_range = coverage_ranges[scenario]
    lines.extend(
        [
            "",
            "| scenario | per-builder implementation coverage range |",
            "| --- | ---: |",
            f"| baseline | {pct_float(before_range[0])}–{pct_float(before_range[1])} |",
            f"| full-audit flags corrected | {pct_float(after_range[0])}–"
            f"{pct_float(after_range[1])} |",
        ]
    )
    return lines


def panel_builder_order(rows: Sequence[Mapping[str, str]], scenario: str) -> dict[str, list[str]]:
    grouped = defaultdict(list)
    for row in rows:
        if row["scenario"] != scenario:
            continue
        grouped[row["panel_id"]].append(row)
    return {
        panel_id: [
            row["builder_id"]
            for row in sorted(panel_rows, key=lambda row: (int(row["rank"]), row["builder_id"]))
        ]
        for panel_id, panel_rows in grouped.items()
    }


def verify_frozen_inputs(
    output_dir: Path,
    grouped: Mapping[tuple[str, str], Sequence[Pair]],
) -> None:
    manifest = json.loads((output_dir / "execution_manifest.json").read_text(encoding="utf-8"))
    current_inputs = {
        "labels": sha256_file(LABELS_PATH),
        **{f"questions_{task_id}": sha256_file(path) for task_id, path in QUESTION_PATHS.items()},
    }
    if current_inputs != manifest["input_sha256"]:
        raise RuntimeError("frozen label/question inputs changed during audit")
    current_artifacts = compute_artifact_hashes(grouped)
    current_combined = combined_artifact_hash(current_artifacts)
    if current_combined != manifest["artifact_tree_sha256"]:
        raise RuntimeError("runs_release artifact tree changed during audit")


def mapped_label(verdict: str) -> str:
    if verdict == "confirmed_gold":
        return "implemented_gold"
    if verdict == "non_gold":
        return "implemented_non_gold"
    if verdict in {"absent", "ambiguous"}:
        return "absent_or_ambiguous"
    raise RuntimeError(f"unmapped verdict: {verdict}")


def verdict_reproduces_label(verdict: str, expected_mapped_label: str) -> bool:
    return mapped_label(verdict) == expected_mapped_label


def format_spans(spans: Sequence[Mapping[str, Any]]) -> str:
    return "|".join(f"{span['file']}:{span['start_line']}-{span['end_line']}" for span in spans)


def join_values(values: Sequence[str]) -> str:
    return "|".join(values)


def bool_text(value: bool) -> str:
    return "true" if value else "false"


def pct(numerator: int, denominator: int) -> str:
    return f"{100.0 * numerator / denominator:.3f}%" if denominator else "n/a"


def pct_float(value: float) -> str:
    return f"{100.0 * value:.3f}%"


def read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as handle:
        return list(csv.DictReader(handle))


def render_csv(rows: Sequence[Mapping[str, Any]], fields: Sequence[str]) -> str:
    import io

    buffer = io.StringIO(newline="")
    writer = csv.DictWriter(buffer, fieldnames=fields, lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
    return buffer.getvalue()


def render_json(value: Any) -> str:
    return json.dumps(value, indent=2, sort_keys=True) + "\n"


def render_jsonl(rows: Sequence[Mapping[str, Any]]) -> str:
    return "".join(
        json.dumps(row, sort_keys=True, ensure_ascii=False) + "\n" for row in rows
    )


def write_csv(
    path: Path,
    rows: Sequence[Mapping[str, Any]],
    fields: Sequence[str],
) -> None:
    write_text(path, render_csv(rows, fields))


def write_json(path: Path, value: Any) -> None:
    write_text(path, render_json(value))


def write_jsonl(path: Path, rows: Sequence[Mapping[str, Any]]) -> None:
    write_text(path, render_jsonl(rows))


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
    temporary.write_text(content, encoding="utf-8")
    temporary.replace(path)


def write_new_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(
        f".{path.name}.new-{os.getpid()}-{os.urandom(8).hex()}"
    )
    try:
        with temporary.open("x", encoding="utf-8") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        # Linking is an atomic create-if-absent operation: it never replaces an
        # immutable evidence record that another process already published.
        os.link(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def write_new_json(path: Path, value: Any) -> None:
    write_new_text(path, render_json(value))


def freeze_text(path: Path, content: str) -> None:
    if path.exists():
        existing = path.read_text(encoding="utf-8")
        if existing != content:
            raise RuntimeError(f"frozen file would change: {path}")
        return
    write_new_text(path, content)


def freeze_json(path: Path, value: Any) -> None:
    freeze_text(path, render_json(value))


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def display_path(path: Path) -> str:
    resolved = path.resolve()
    try:
        return resolved.relative_to(ROOT).as_posix()
    except ValueError:
        return str(resolved)


def utc_now() -> str:
    return datetime.now(UTC).isoformat()


def elapsed_seconds(started_at: str) -> float:
    start = datetime.fromisoformat(started_at)
    return (datetime.now(UTC) - start).total_seconds()


if __name__ == "__main__":
    main()
