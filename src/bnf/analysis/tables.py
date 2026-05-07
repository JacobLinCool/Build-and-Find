"""Canonical analysis tables for Build-and-Find run records.

Raw ``runs/records/*.json`` files remain the append-only source of truth.
This module materializes those records into stable CSV tables that every
paper-analysis script can read with a shared schema:

* ``build_runs.csv``: one row per build run.
* ``find_runs.csv``: one row per finder invocation.
* ``question_answers.csv``: one row per answered question in a finder run.
* ``panel_membership.csv``: normalized panel memberships for filtering.
"""

from __future__ import annotations

import csv
import hashlib
import json
import os
from collections.abc import Iterable
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from bnf.schema.question import QuestionBank, load_question_bank
from bnf.schema.run import BuildRun, FindRun

TABLE_SCHEMA_VERSION = "2.3"

SCRATCH_TASKS = frozenset({"scratch_minidb", "scratch_nanoweb"})
PAPER_FINAL_PANEL_ID = "paper_final_scratch"
HIGH_EFFORT_PANEL_ID = "high_effort_panel"
LOW_EFFORT_PANEL_ID = "low_effort_panel"
CROSS_EFFORT_PANEL_ID = "cross_effort_panel"
MIMO_EXTENSION_PANEL_ID = "mimo_extension"
CONTROL_PANEL_ID = "control_all"
BUILD_FIDELITY_DIAGNOSTIC_PANEL_ID = "build_fidelity_diagnostic"
ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID = "artifact_present_diagnostic"
EMPTY_ARTIFACT_MARKER = ".bnf-empty-artifact"

HIGH_PAPER_AGENTS = (
    "claude_code_opus_4_7_high",
    "claude_code_sonnet_4_6_high",
    "codex_gpt5_5_high",
    "codex_gpt5_4_mini_high",
)
LOW_PAPER_AGENTS = (
    "claude_code_opus_4_7_low",
    "claude_code_sonnet_4_6_low",
    "codex_gpt5_5_low",
    "codex_gpt5_4_mini_low",
)
MIMO_PAPER_AGENTS = (
    "claude_code_mimo_v2_5_pro_high",
    "claude_code_mimo_v2_5_pro_low",
    "claude_code_mimo_v2_5_high",
    "claude_code_mimo_v2_5_low",
)
PAPER_MAIN_AGENTS = frozenset((*HIGH_PAPER_AGENTS, *LOW_PAPER_AGENTS, *MIMO_PAPER_AGENTS))

TABLE_NAMES = (
    "build_runs",
    "find_runs",
    "question_answers",
    "panel_membership",
)

BUILD_RUN_COLUMNS = (
    "build_run_id",
    "task_id",
    "builder_id",
    "builder_adapter",
    "builder_model_id",
    "builder_family",
    "builder_effort",
    "build_trial_id",
    "build_seed",
    "build_status",
    "build_error",
    "compile_pass",
    "rust_files",
    "rust_loc",
    "artifact_path",
    "artifact_exists",
    "artifact_hash",
    "build_efficiency_present",
    "build_retrieval_bytes",
    "build_tool_call_bytes",
    "build_total_bytes",
    "build_tool_call_count",
    "build_tool_result_count",
    "build_turn_count",
    "build_wall_clock_seconds",
    "build_vendor_total_input_tokens",
    "build_vendor_cached_input_tokens",
    "build_vendor_uncached_input_tokens",
    "build_vendor_cache_creation_input_tokens",
    "build_vendor_non_reasoning_output_tokens",
    "build_vendor_reasoning_output_tokens",
    "build_vendor_total_tokens",
    "spec_hash",
    "build_prompt_hash",
    "build_env_hash",
    "build_created_at",
    "bnf_version",
    "panel_ids",
)

FIND_RUN_COLUMNS = (
    "find_run_id",
    "build_run_id",
    "task_id",
    "builder_id",
    "builder_adapter",
    "builder_model_id",
    "builder_family",
    "builder_effort",
    "build_trial_id",
    "build_seed",
    "build_status",
    "compile_pass",
    "rust_files",
    "rust_loc",
    "artifact_hash",
    "finder_id",
    "finder_adapter",
    "finder_model_id",
    "finder_family",
    "finder_effort",
    "finder_trial_id",
    "finder_seed",
    "find_status",
    "find_error",
    "n_questions",
    "n_answered",
    "n_correct",
    "accuracy",
    "perfect_run",
    "efficiency_present",
    "retrieval_bytes",
    "tool_call_bytes",
    "total_bytes",
    "metric_source",
    "session_log_retrieval_bytes",
    "session_log_tool_call_bytes",
    "session_log_total_bytes",
    "tool_call_count",
    "tool_result_count",
    "turn_count",
    "wall_clock_seconds",
    "generation_tok_per_s",
    "request_count",
    "proxy_session_id",
    "vendor_total_input_tokens",
    "vendor_cached_input_tokens",
    "vendor_uncached_input_tokens",
    "vendor_cache_creation_input_tokens",
    "vendor_non_reasoning_output_tokens",
    "vendor_reasoning_output_tokens",
    "vendor_total_tokens",
    "spec_hash",
    "find_prompt_hash",
    "find_env_hash",
    "find_created_at",
    "bnf_version",
    "panel_ids",
)

QUESTION_ANSWER_COLUMNS = (
    "question_row_id",
    "find_run_id",
    "build_run_id",
    "task_id",
    "builder_id",
    "build_trial_id",
    "finder_id",
    "finder_trial_id",
    "question_id",
    "question_type",
    "intention_id",
    "source_requirement_id",
    "source_intention_id",
    "selected_option",
    "correct_option",
    "selected_text",
    "correct_text",
    "answered",
    "is_correct",
    "evidence_files",
    "evidence_symbols",
    "rationale",
    "panel_ids",
)

PANEL_MEMBERSHIP_COLUMNS = (
    "panel_id",
    "table_name",
    "row_id",
)

NUMERIC_FIELDS = frozenset(
    {
        "build_trial_id",
        "build_seed",
        "rust_files",
        "rust_loc",
        "finder_trial_id",
        "finder_seed",
        "n_questions",
        "n_answered",
        "n_correct",
        "accuracy",
        "retrieval_bytes",
        "tool_call_bytes",
        "total_bytes",
        "session_log_retrieval_bytes",
        "session_log_tool_call_bytes",
        "session_log_total_bytes",
        "tool_call_count",
        "tool_result_count",
        "turn_count",
        "wall_clock_seconds",
        "generation_tok_per_s",
        "request_count",
        "build_retrieval_bytes",
        "build_tool_call_bytes",
        "build_total_bytes",
        "build_tool_call_count",
        "build_tool_result_count",
        "build_turn_count",
        "build_wall_clock_seconds",
        "build_vendor_total_input_tokens",
        "build_vendor_cached_input_tokens",
        "build_vendor_uncached_input_tokens",
        "build_vendor_cache_creation_input_tokens",
        "build_vendor_non_reasoning_output_tokens",
        "build_vendor_reasoning_output_tokens",
        "build_vendor_total_tokens",
        "vendor_total_input_tokens",
        "vendor_cached_input_tokens",
        "vendor_uncached_input_tokens",
        "vendor_cache_creation_input_tokens",
        "vendor_non_reasoning_output_tokens",
        "vendor_reasoning_output_tokens",
        "vendor_total_tokens",
    }
)

BOOLEAN_FIELDS = frozenset(
    {
        "compile_pass",
        "artifact_exists",
        "build_efficiency_present",
        "perfect_run",
        "efficiency_present",
        "answered",
        "is_correct",
    }
)


def agent_family(agent_id: str) -> str:
    if agent_id.startswith("control_"):
        return "control"
    if "mimo" in agent_id:
        return "mimo"
    if agent_id.startswith("claude_code_"):
        return "claude"
    if agent_id.startswith("codex_"):
        return "codex"
    return "unknown"


def agent_effort(agent_id: str) -> str:
    if agent_id.endswith("_high"):
        return "high"
    return "low" if agent_id.endswith("_low") else "default"


def load_build_records(records_dir: Path) -> list[BuildRun]:
    paths = [*records_dir.glob("build-*.json"), *records_dir.glob("control-build-*.json")]
    return [
        BuildRun.model_validate_json(path.read_text(encoding="utf-8"))
        for path in sorted(paths)
    ]


def load_find_records(records_dir: Path) -> list[FindRun]:
    paths = [*records_dir.glob("find-*.json"), *records_dir.glob("control-find-*.json")]
    return [
        FindRun.model_validate_json(path.read_text(encoding="utf-8"))
        for path in sorted(paths)
    ]


def export_analysis_tables(
    *,
    records_dir: Path,
    out_dir: Path,
    questions_dir: Path | None = None,
) -> dict[str, Any]:
    """Materialize raw run records into canonical CSV analysis tables."""

    build_runs = [run for run in load_build_records(records_dir) if run.task_id in SCRATCH_TASKS]
    find_runs = [run for run in load_find_records(records_dir) if run.task_id in SCRATCH_TASKS]
    builds_by_id = {run.build_run_id: run for run in build_runs}
    missing = sorted({run.build_run_id for run in find_runs} - set(builds_by_id))
    if missing:
        sample = ", ".join(missing[:5])
        raise ValueError(f"{len(missing)} find runs have no build record: {sample}")

    question_banks = _load_question_banks(questions_dir)
    artifact_hashes: dict[str, str] = {}

    build_rows = [
        _build_run_row(run, artifact_hashes=artifact_hashes)
        for run in build_runs
    ]
    find_rows: list[dict[str, Any]] = []
    question_rows: list[dict[str, Any]] = []
    membership_rows: list[dict[str, str]] = []

    for run in find_runs:
        build = builds_by_id[run.build_run_id]
        find_row = _find_run_row(run, build, artifact_hashes=artifact_hashes)
        find_rows.append(find_row)
        _extend_memberships(
            membership_rows,
            table_name="find_runs",
            row_id=run.find_run_id,
            panel_ids=_split_panel_ids(find_row["panel_ids"]),
        )

        question_meta = _question_meta_for_run(
            question_banks,
            task_id=run.task_id,
            questions_dir=questions_dir,
        )
        for answer in run.answers:
            question_row = _question_answer_row(
                run,
                build,
                answer,
                question_meta=question_meta,
                run_panel_ids=find_row["panel_ids"],
            )
            question_rows.append(question_row)
            _extend_memberships(
                membership_rows,
                table_name="question_answers",
                row_id=question_row["question_row_id"],
                panel_ids=_split_panel_ids(question_row["panel_ids"]),
            )

    build_panel_ids = _build_panel_ids_from_find_rows(find_rows)
    for row in build_rows:
        panels = (
            *_base_panel_ids_for_task(row["task_id"]),
            *_build_diagnostic_panel_ids(row),
            *build_panel_ids.get(row["build_run_id"], ()),
        )
        row["panel_ids"] = _join_panel_ids(panels)
        _extend_memberships(
            membership_rows,
            table_name="build_runs",
            row_id=row["build_run_id"],
            panel_ids=panels,
        )

    out_dir.mkdir(parents=True, exist_ok=True)
    _write_csv_atomic(out_dir / "build_runs.csv", BUILD_RUN_COLUMNS, build_rows)
    _write_csv_atomic(out_dir / "find_runs.csv", FIND_RUN_COLUMNS, find_rows)
    _write_csv_atomic(out_dir / "question_answers.csv", QUESTION_ANSWER_COLUMNS, question_rows)
    _write_csv_atomic(
        out_dir / "panel_membership.csv",
        PANEL_MEMBERSHIP_COLUMNS,
        sorted(membership_rows, key=lambda r: (r["panel_id"], r["table_name"], r["row_id"])),
    )

    metadata = _metadata_payload(
        records_dir=records_dir,
        out_dir=out_dir,
        build_rows=build_rows,
        find_rows=find_rows,
        question_rows=question_rows,
    )
    _write_json_atomic(out_dir / "experiments.json", metadata)
    manifest = _manifest_payload(
        out_dir=out_dir,
        generated_at=metadata["generated_at"],
        row_counts={
            "build_runs": len(build_rows),
            "find_runs": len(find_rows),
            "question_answers": len(question_rows),
            "panel_membership": len(membership_rows),
        },
    )
    _write_json_atomic(out_dir / "MANIFEST.json", manifest)
    return metadata


def load_find_run_table(table_dir: Path, *, panel_id: str | None = None) -> list[dict[str, Any]]:
    rows = _read_csv_typed(table_dir / "find_runs.csv")
    if panel_id is None:
        return rows
    return [row for row in rows if panel_id in _split_panel_ids(row.get("panel_ids", ""))]


def load_question_answer_table(
    table_dir: Path,
    *,
    panel_id: str | None = None,
) -> list[dict[str, Any]]:
    rows = _read_csv_typed(table_dir / "question_answers.csv")
    if panel_id is None:
        return rows
    return [row for row in rows if panel_id in _split_panel_ids(row.get("panel_ids", ""))]


def load_build_run_table(table_dir: Path, *, panel_id: str | None = None) -> list[dict[str, Any]]:
    rows = _read_csv_typed(table_dir / "build_runs.csv")
    if panel_id is None:
        return rows
    return [row for row in rows if panel_id in _split_panel_ids(row.get("panel_ids", ""))]


def require_analysis_tables(table_dir: Path) -> None:
    missing = [name for name in TABLE_NAMES if not (table_dir / f"{name}.csv").exists()]
    if missing:
        missing_list = ", ".join(f"{name}.csv" for name in missing)
        raise FileNotFoundError(
            f"missing canonical analysis table(s): {missing_list}. "
            "Run `uv run python scripts/export_analysis_tables.py` first."
        )


def _build_run_row(run: BuildRun, *, artifact_hashes: dict[str, str]) -> dict[str, Any]:
    agent = run.manifest.agent
    artifact_path = Path(run.artifact_path)
    artifact_exists = artifact_path.exists()
    artifact_hash = _artifact_hash_cached(artifact_path, artifact_hashes) if artifact_exists else ""
    efficiency = run.usage.efficiency
    vendor_tokens = efficiency.vendor_tokens if efficiency else None
    return {
        "build_run_id": run.build_run_id,
        "task_id": run.task_id,
        "builder_id": agent.id,
        "builder_adapter": agent.adapter,
        "builder_model_id": agent.model_id,
        "builder_family": agent_family(agent.id),
        "builder_effort": agent_effort(agent.id),
        "build_trial_id": run.manifest.trial_id,
        "build_seed": run.manifest.seed,
        "build_status": run.status,
        "build_error": run.error or "",
        "compile_pass": run.validation.compile_pass,
        "rust_files": run.validation.rust_files,
        "rust_loc": run.validation.rust_loc,
        "artifact_path": str(artifact_path),
        "artifact_exists": artifact_exists,
        "artifact_hash": artifact_hash,
        "build_efficiency_present": efficiency is not None,
        "build_retrieval_bytes": efficiency.novel_retrieval_bytes if efficiency else "",
        "build_tool_call_bytes": efficiency.novel_tool_call_bytes if efficiency else "",
        "build_total_bytes": (
            efficiency.novel_retrieval_bytes + efficiency.novel_tool_call_bytes
            if efficiency
            else ""
        ),
        "build_tool_call_count": efficiency.novel_tool_call_count if efficiency else "",
        "build_tool_result_count": efficiency.novel_tool_result_count if efficiency else "",
        "build_turn_count": efficiency.turn_count if efficiency else "",
        "build_wall_clock_seconds": (
            efficiency.wall_clock_seconds if efficiency else run.usage.wall_clock_seconds
        ),
        "build_vendor_total_input_tokens": (
            vendor_tokens.total_input_tokens if vendor_tokens else ""
        ),
        "build_vendor_cached_input_tokens": (
            vendor_tokens.cached_input_tokens if vendor_tokens else ""
        ),
        "build_vendor_uncached_input_tokens": (
            vendor_tokens.uncached_input_tokens if vendor_tokens else ""
        ),
        "build_vendor_cache_creation_input_tokens": (
            vendor_tokens.cache_creation_input_tokens if vendor_tokens else ""
        ),
        "build_vendor_non_reasoning_output_tokens": (
            vendor_tokens.non_reasoning_output_tokens if vendor_tokens else ""
        ),
        "build_vendor_reasoning_output_tokens": (
            vendor_tokens.reasoning_output_tokens if vendor_tokens else ""
        ),
        "build_vendor_total_tokens": vendor_tokens.total_tokens if vendor_tokens else "",
        "spec_hash": run.manifest.spec_hash,
        "build_prompt_hash": run.manifest.prompt_hash,
        "build_env_hash": run.manifest.env_hash,
        "build_created_at": run.manifest.created_at,
        "bnf_version": run.manifest.bnf_version,
        "panel_ids": "",
    }


def _find_run_row(
    run: FindRun,
    build: BuildRun,
    *,
    artifact_hashes: dict[str, str],
) -> dict[str, Any]:
    builder = build.manifest.agent
    finder = run.manifest.agent
    n_questions = len(run.answers)
    n_answered = sum(1 for answer in run.answers if answer.selected_option is not None)
    n_correct = sum(1 for answer in run.answers if answer.is_correct)
    accuracy = n_correct / n_questions if n_questions else 0.0
    efficiency = run.usage.efficiency
    vendor_tokens = efficiency.vendor_tokens if efficiency else None
    artifact_path = Path(build.artifact_path)
    artifact_hash = (
        _artifact_hash_cached(artifact_path, artifact_hashes) if artifact_path.exists() else ""
    )

    retrieval_bytes = efficiency.novel_retrieval_bytes if efficiency else ""
    tool_call_bytes = efficiency.novel_tool_call_bytes if efficiency else ""
    total_bytes = (
        efficiency.novel_retrieval_bytes + efficiency.novel_tool_call_bytes
        if efficiency
        else ""
    )
    session_log_retrieval_bytes = run.usage.retrieval_payload_bytes
    session_log_tool_call_bytes = run.usage.tool_input_bytes
    session_log_total_bytes = session_log_retrieval_bytes + session_log_tool_call_bytes
    wall_clock = efficiency.wall_clock_seconds if efficiency else run.usage.wall_clock_seconds

    panel_ids = _panel_ids_for_find_run(
        run.task_id,
        builder.id,
        finder.id,
        build_is_artifact_present=_build_is_artifact_present(build),
        build_is_primary_eligible=_build_is_primary_eligible(build),
    )

    return {
        "find_run_id": run.find_run_id,
        "build_run_id": run.build_run_id,
        "task_id": run.task_id,
        "builder_id": builder.id,
        "builder_adapter": builder.adapter,
        "builder_model_id": builder.model_id,
        "builder_family": agent_family(builder.id),
        "builder_effort": agent_effort(builder.id),
        "build_trial_id": build.manifest.trial_id,
        "build_seed": build.manifest.seed,
        "build_status": build.status,
        "compile_pass": build.validation.compile_pass,
        "rust_files": build.validation.rust_files,
        "rust_loc": build.validation.rust_loc,
        "artifact_hash": artifact_hash,
        "finder_id": finder.id,
        "finder_adapter": finder.adapter,
        "finder_model_id": finder.model_id,
        "finder_family": agent_family(finder.id),
        "finder_effort": agent_effort(finder.id),
        "finder_trial_id": run.manifest.trial_id,
        "finder_seed": run.manifest.seed,
        "find_status": run.status,
        "find_error": run.error or "",
        "n_questions": n_questions,
        "n_answered": n_answered,
        "n_correct": n_correct,
        "accuracy": accuracy,
        "perfect_run": n_questions > 0 and n_correct == n_questions,
        "efficiency_present": efficiency is not None,
        "retrieval_bytes": retrieval_bytes,
        "tool_call_bytes": tool_call_bytes,
        "total_bytes": total_bytes,
        "metric_source": "proxy_novel_usage_events" if efficiency else "missing_proxy_efficiency",
        "session_log_retrieval_bytes": session_log_retrieval_bytes,
        "session_log_tool_call_bytes": session_log_tool_call_bytes,
        "session_log_total_bytes": session_log_total_bytes,
        "tool_call_count": efficiency.novel_tool_call_count if efficiency else "",
        "tool_result_count": efficiency.novel_tool_result_count if efficiency else "",
        "turn_count": efficiency.turn_count if efficiency else "",
        "wall_clock_seconds": wall_clock,
        "generation_tok_per_s": efficiency.generation_tok_per_s if efficiency else "",
        "request_count": efficiency.request_count if efficiency else "",
        "proxy_session_id": efficiency.proxy_session_id if efficiency else "",
        "vendor_total_input_tokens": vendor_tokens.total_input_tokens if vendor_tokens else "",
        "vendor_cached_input_tokens": vendor_tokens.cached_input_tokens if vendor_tokens else "",
        "vendor_uncached_input_tokens": vendor_tokens.uncached_input_tokens if vendor_tokens else "",
        "vendor_cache_creation_input_tokens": (
            vendor_tokens.cache_creation_input_tokens if vendor_tokens else ""
        ),
        "vendor_non_reasoning_output_tokens": (
            vendor_tokens.non_reasoning_output_tokens if vendor_tokens else ""
        ),
        "vendor_reasoning_output_tokens": (
            vendor_tokens.reasoning_output_tokens if vendor_tokens else ""
        ),
        "vendor_total_tokens": vendor_tokens.total_tokens if vendor_tokens else "",
        "spec_hash": run.manifest.spec_hash,
        "find_prompt_hash": run.manifest.prompt_hash,
        "find_env_hash": run.manifest.env_hash,
        "find_created_at": run.manifest.created_at,
        "bnf_version": run.manifest.bnf_version,
        "panel_ids": _join_panel_ids(panel_ids),
    }


def _question_answer_row(
    run: FindRun,
    build: BuildRun,
    answer: Any,
    *,
    question_meta: dict[str, Any] | None,
    run_panel_ids: str,
) -> dict[str, Any]:
    question = question_meta.get(answer.question_id) if question_meta else None
    if question_meta is not None and question is None:
        raise ValueError(
            f"{run.find_run_id} references unknown question_id {answer.question_id!r}"
        )
    selected_text = (
        answer.presented_options.get(answer.selected_option, "")
        if answer.selected_option is not None
        else ""
    )
    correct_text = answer.presented_options.get(answer.presented_correct_key, "")
    builder = build.manifest.agent
    finder = run.manifest.agent
    return {
        "question_row_id": f"{run.find_run_id}::{answer.question_id}",
        "find_run_id": run.find_run_id,
        "build_run_id": run.build_run_id,
        "task_id": run.task_id,
        "builder_id": builder.id,
        "build_trial_id": build.manifest.trial_id,
        "finder_id": finder.id,
        "finder_trial_id": run.manifest.trial_id,
        "question_id": answer.question_id,
        "question_type": question.get("question_type", "") if question else "",
        "intention_id": question.get("intention_id", "") if question else "",
        "source_requirement_id": question.get("source_requirement_id", "") if question else "",
        "source_intention_id": question.get("source_intention_id", "") if question else "",
        "selected_option": answer.selected_option or "",
        "correct_option": answer.presented_correct_key,
        "selected_text": selected_text,
        "correct_text": correct_text,
        "answered": answer.selected_option is not None,
        "is_correct": answer.is_correct,
        "evidence_files": json.dumps(answer.evidence_files, ensure_ascii=False),
        "evidence_symbols": json.dumps(answer.evidence_symbols, ensure_ascii=False),
        "rationale": answer.rationale or "",
        "panel_ids": run_panel_ids,
    }


def _load_question_banks(questions_dir: Path | None) -> dict[str, dict[str, Any]]:
    if questions_dir is None:
        return {}
    if not questions_dir.exists():
        raise FileNotFoundError(f"question-bank directory does not exist: {questions_dir}")
    banks: dict[str, dict[str, Any]] = {}
    for path in sorted(questions_dir.glob("*.yaml")):
        bank: QuestionBank = load_question_bank(path)
        banks[bank.task_id] = {
            question.question_id: {
                "question_type": question.question_type,
                "intention_id": question.intention_id,
                "source_requirement_id": question.source_trace.source_requirement_id,
                "source_intention_id": question.source_trace.source_intention_id,
            }
            for question in bank.questions
        }
    return banks


def _question_meta_for_run(
    question_banks: dict[str, dict[str, Any]],
    *,
    task_id: str,
    questions_dir: Path | None,
) -> dict[str, Any] | None:
    if questions_dir is None:
        return None
    question_meta = question_banks.get(task_id)
    if question_meta is None:
        raise ValueError(f"no question bank for task_id {task_id!r} in {questions_dir}")
    return question_meta


def _base_panel_ids_for_task(task_id: str) -> tuple[str, ...]:
    panels = ["all_records"]
    if task_id in SCRATCH_TASKS:
        panels.append("scratch_all")
    return tuple(panels)


def _build_is_primary_eligible(build: BuildRun) -> bool:
    return _build_is_artifact_present(build) and build.validation.compile_pass


def _build_is_artifact_present(build: BuildRun) -> bool:
    return build.status == "ok" and Path(build.artifact_path).exists()


def _panel_ids_for_find_run(
    task_id: str,
    builder_id: str,
    finder_id: str,
    *,
    build_is_artifact_present: bool,
    build_is_primary_eligible: bool,
) -> tuple[str, ...]:
    panels = list(_base_panel_ids_for_task(task_id))
    if builder_id.startswith("control_"):
        panels.extend((CONTROL_PANEL_ID, builder_id.removeprefix("control_")))
        return tuple(panels)
    if task_id in SCRATCH_TASKS:
        if not build_is_artifact_present:
            panels.append(BUILD_FIDELITY_DIAGNOSTIC_PANEL_ID)
            return tuple(panels)
        if builder_id in PAPER_MAIN_AGENTS and finder_id in PAPER_MAIN_AGENTS:
            panels.append(ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID)
            if build_is_primary_eligible:
                panels.append(PAPER_FINAL_PANEL_ID)
                if agent_effort(builder_id) == "high" and agent_effort(finder_id) == "high":
                    panels.append(HIGH_EFFORT_PANEL_ID)
                if agent_effort(builder_id) == "low" and agent_effort(finder_id) == "low":
                    panels.append(LOW_EFFORT_PANEL_ID)
                if agent_effort(builder_id) in {"high", "low"} and agent_effort(
                    finder_id
                ) in {"high", "low"} and agent_effort(builder_id) != agent_effort(finder_id):
                    panels.append(CROSS_EFFORT_PANEL_ID)
                if "mimo" in builder_id or "mimo" in finder_id:
                    panels.append(MIMO_EXTENSION_PANEL_ID)
    return tuple(panels)


def _build_panel_ids_from_find_rows(find_rows: Iterable[dict[str, Any]]) -> dict[str, tuple[str, ...]]:
    panels_by_build: dict[str, set[str]] = {}
    for row in find_rows:
        build_run_id = str(row["build_run_id"])
        panels_by_build.setdefault(build_run_id, set()).update(_split_panel_ids(row["panel_ids"]))
    return {build_id: tuple(sorted(panels)) for build_id, panels in panels_by_build.items()}


def _build_diagnostic_panel_ids(row: dict[str, Any]) -> tuple[str, ...]:
    if row["task_id"] not in SCRATCH_TASKS or str(row["builder_id"]).startswith("control_"):
        return ()
    if row["build_status"] == "ok" and not bool(row["artifact_exists"]):
        return (BUILD_FIDELITY_DIAGNOSTIC_PANEL_ID,)
    return ()


def _metadata_payload(
    *,
    records_dir: Path,
    out_dir: Path,
    build_rows: list[dict[str, Any]],
    find_rows: list[dict[str, Any]],
    question_rows: list[dict[str, Any]],
) -> dict[str, Any]:
    generated_at = _source_snapshot_at(build_rows, find_rows)
    panels = {}
    known_panels = sorted(
        {
            panel_id
            for row in (*build_rows, *find_rows, *question_rows)
            for panel_id in _split_panel_ids(row.get("panel_ids", ""))
        }
    )
    for panel_id in known_panels:
        panel_find_rows = [row for row in find_rows if panel_id in _split_panel_ids(row["panel_ids"])]
        panel_question_rows = [
            row for row in question_rows if panel_id in _split_panel_ids(row["panel_ids"])
        ]
        panel_build_ids = {
            row["build_run_id"]
            for row in build_rows
            if panel_id in _split_panel_ids(row.get("panel_ids", ""))
        }
        panels[panel_id] = {
            "build_run_count": len(panel_build_ids),
            "find_run_count": len(panel_find_rows),
            "question_row_count": len(panel_question_rows),
            "task_ids": sorted({row["task_id"] for row in panel_find_rows}),
            "builder_ids": sorted({row["builder_id"] for row in panel_find_rows}),
            "finder_ids": sorted({row["finder_id"] for row in panel_find_rows}),
        }

    return {
        "schema_version": TABLE_SCHEMA_VERSION,
        "generated_at": generated_at,
        "records_dir": _portable_path(records_dir),
        "tables_dir": _portable_path(out_dir),
        "tables": {
            "build_runs": {"path": "build_runs.csv", "columns": list(BUILD_RUN_COLUMNS)},
            "find_runs": {"path": "find_runs.csv", "columns": list(FIND_RUN_COLUMNS)},
            "question_answers": {
                "path": "question_answers.csv",
                "columns": list(QUESTION_ANSWER_COLUMNS),
            },
            "panel_membership": {
                "path": "panel_membership.csv",
                "columns": list(PANEL_MEMBERSHIP_COLUMNS),
            },
        },
        "panels": panels,
        "panel_definitions": _panel_definitions(),
    }


def _panel_definitions() -> dict[str, str]:
    return {
        "all_records": "All scratch-task release records, including diagnostics and controls.",
        "scratch_all": "All release records for scratch_minidb and scratch_nanoweb.",
        PAPER_FINAL_PANEL_ID: (
            "Primary paper panel: scratch tasks, final paper agents, status-ok "
            "builds whose artifact path exists and whose compile probe passed."
        ),
        ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID: (
            "Artifact-present diagnostic panel: scratch tasks, final paper agents, "
            "status-ok builds whose artifact path exists, including compile-probe "
            "failures retained for validity-stress diagnostics."
        ),
        HIGH_EFFORT_PANEL_ID: "Primary high-effort slice of paper_final_scratch.",
        LOW_EFFORT_PANEL_ID: "Primary low-effort slice of paper_final_scratch.",
        CROSS_EFFORT_PANEL_ID: "Cross-effort diagnostic slice of paper_final_scratch.",
        MIMO_EXTENSION_PANEL_ID: "MIMO rows inside paper_final_scratch.",
        CONTROL_PANEL_ID: "Spec-only and question-only control runs.",
        BUILD_FIDELITY_DIAGNOSTIC_PANEL_ID: (
            "Status-ok scratch builds excluded from primary panels because the "
            "release artifact path is missing, plus their associated finder diagnostics."
        ),
    }


def _manifest_payload(
    *,
    out_dir: Path,
    generated_at: str,
    row_counts: dict[str, int],
) -> dict[str, Any]:
    files = {}
    for path in sorted(out_dir.iterdir()):
        if path.name == "MANIFEST.json" or not path.is_file():
            continue
        rel = path.name
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        table_name = path.stem if path.suffix == ".csv" else None
        files[rel] = {
            "sha256": digest,
            "bytes": path.stat().st_size,
            "row_count": row_counts.get(table_name, "") if table_name else "",
        }
    return {
        "schema_version": TABLE_SCHEMA_VERSION,
        "generated_at": generated_at,
        "table_dir": _portable_path(out_dir),
        "files": files,
    }


def _source_snapshot_at(build_rows: list[dict[str, Any]], find_rows: list[dict[str, Any]]) -> str:
    timestamps = [
        str(row[key])
        for row in (*build_rows, *find_rows)
        for key in ("build_created_at", "find_created_at")
        if row.get(key)
    ]
    if timestamps:
        return max(timestamps)
    return datetime.fromtimestamp(0, timezone.utc).isoformat()


def _portable_path(path: Path) -> str:
    resolved = path.resolve()
    try:
        return resolved.relative_to(Path.cwd().resolve()).as_posix()
    except ValueError:
        return resolved.as_posix()


def _artifact_hash_cached(path: Path, cache: dict[str, str]) -> str:
    key = str(path.resolve())
    if key not in cache:
        cache[key] = artifact_tree_hash(path)
    return cache[key]


def artifact_tree_hash(path: Path) -> str:
    """Return a stable hash of source artifact contents."""

    digest = hashlib.sha256()
    for file_path in sorted(_iter_artifact_files(path)):
        rel = file_path.relative_to(path).as_posix()
        digest.update(rel.encode("utf-8"))
        digest.update(b"\0")
        digest.update(file_path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()[:16]


def _iter_artifact_files(path: Path) -> Iterable[Path]:
    skip_dirs = {".git", "target", "node_modules"}
    for child in path.rglob("*"):
        if child.is_dir():
            continue
        if child.name == EMPTY_ARTIFACT_MARKER:
            continue
        rel_parts = child.relative_to(path).parts
        if any(part in skip_dirs for part in rel_parts):
            continue
        yield child


def _write_csv_atomic(path: Path, columns: tuple[str, ...], rows: Iterable[dict[str, Any]]) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    with tmp.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(columns), extrasaction="raise")
        writer.writeheader()
        for row in rows:
            writer.writerow({column: _csv_value(row.get(column, "")) for column in columns})
    os.replace(tmp, path)


def _write_json_atomic(path: Path, payload: dict[str, Any]) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")
    os.replace(tmp, path)


def _csv_value(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, bool):
        return "true" if value else "false"
    return str(value)


def _read_csv_typed(path: Path) -> list[dict[str, Any]]:
    require_analysis_tables(path.parent)
    with path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        return [_typed_row(row) for row in reader]


def _typed_row(row: dict[str, str]) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for key, value in row.items():
        if key in BOOLEAN_FIELDS:
            out[key] = _to_bool(value)
        elif key in NUMERIC_FIELDS:
            out[key] = _to_number(value)
        else:
            out[key] = value
    return out


def _to_bool(value: str) -> bool:
    return value.strip().lower() == "true"


def _to_number(value: str) -> int | float | None:
    if value == "":
        return None
    if any(ch in value for ch in ".eE"):
        return float(value)
    return int(value)


def _extend_memberships(
    rows: list[dict[str, str]],
    *,
    table_name: str,
    row_id: str,
    panel_ids: Iterable[str],
) -> None:
    for panel_id in dict.fromkeys(panel_ids):
        rows.append({"panel_id": panel_id, "table_name": table_name, "row_id": row_id})


def _join_panel_ids(panel_ids: Iterable[str]) -> str:
    return "|".join(sorted(dict.fromkeys(panel_ids)))


def _split_panel_ids(panel_ids: str | Iterable[str]) -> tuple[str, ...]:
    if isinstance(panel_ids, str):
        if not panel_ids:
            return ()
        return tuple(part for part in panel_ids.split("|") if part)
    return tuple(panel_ids)
