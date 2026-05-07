from __future__ import annotations

import json
from pathlib import Path

import pytest

from bnf.analysis.tables import (
    ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID,
    BUILD_FIDELITY_DIAGNOSTIC_PANEL_ID,
    CONTROL_PANEL_ID,
    PAPER_FINAL_PANEL_ID,
    export_analysis_tables,
    load_build_run_table,
    load_find_run_table,
    load_question_answer_table,
)
from bnf.schema.run import (
    AgentInfo,
    BuildRun,
    Efficiency,
    FinderAnswer,
    FindRun,
    RunManifest,
    UsageMetrics,
    ValidationResult,
    VendorTokens,
    VendorUsage,
    write_run,
)


def _agent(agent_id: str) -> AgentInfo:
    if agent_id.startswith("claude_code_"):
        return AgentInfo(id=agent_id, adapter="claude_code", model_id="model")
    return AgentInfo(id=agent_id, adapter="codex", model_id="model")


def _manifest(run_id: str, agent_id: str, *, seed: int = 0, trial_id: int = 0) -> RunManifest:
    return RunManifest(
        run_id=run_id,
        spec_hash="spec-hash",
        prompt_hash="prompt-hash",
        agent=_agent(agent_id),
        seed=seed,
        trial_id=trial_id,
        env_hash="env-hash",
        created_at="2026-01-01T00:00:00+00:00",
    )


def _build_run(
    run_id: str,
    task_id: str,
    builder_id: str,
    artifact_path: Path,
    *,
    compile_pass: bool = True,
    trial_id: int = 0,
) -> BuildRun:
    return BuildRun(
        build_run_id=run_id,
        task_id=task_id,
        manifest=_manifest(run_id, builder_id, seed=11, trial_id=trial_id),
        artifact_path=str(artifact_path),
        usage=UsageMetrics(
            efficiency=Efficiency(
                novel_retrieval_bytes=5,
                novel_tool_call_bytes=6,
                novel_tool_call_count=1,
                novel_tool_result_count=2,
                wall_clock_seconds=3.5,
                vendor_tokens=VendorTokens(total_input_tokens=10, total_tokens=14),
            )
        ),
        validation=ValidationResult(compile_pass=compile_pass, rust_files=2, rust_loc=42),
        status="ok",
    )


def _find_run(
    run_id: str,
    build_run_id: str,
    task_id: str,
    finder_id: str,
    *,
    trial_id: int = 0,
) -> FindRun:
    return FindRun(
        find_run_id=run_id,
        build_run_id=build_run_id,
        task_id=task_id,
        manifest=_manifest(run_id, finder_id, seed=13, trial_id=trial_id),
        answers=[
            FinderAnswer(
                question_id="q1",
                presented_options={"A": "alpha", "B": "beta", "C": "gamma", "D": "delta"},
                presented_correct_key="A",
                selected_option="A",
                is_correct=True,
                evidence_files=["src/lib.rs"],
                evidence_symbols=["answer"],
                rationale="found in code",
            )
        ],
        usage=UsageMetrics(
            retrieval_payload_bytes=111,
            tool_input_bytes=22,
            tool_calls=2,
            wall_clock_seconds=9.0,
            vendor_usage=VendorUsage(total_tokens=999),
            efficiency=Efficiency(
                novel_retrieval_bytes=700,
                novel_tool_call_bytes=33,
                novel_tool_call_count=4,
                novel_tool_result_count=5,
                turn_count=6,
                wall_clock_seconds=8.0,
                generation_tok_per_s=10.5,
                request_count=2,
                proxy_session_id="proxy-session",
                vendor_tokens=VendorTokens(
                    total_input_tokens=100,
                    cached_input_tokens=30,
                    uncached_input_tokens=70,
                    non_reasoning_output_tokens=20,
                    reasoning_output_tokens=5,
                    total_tokens=125,
                ),
            ),
        ),
        status="ok",
    )


def _write_question_bank(questions_dir: Path, task_id: str = "scratch_minidb") -> None:
    questions_dir.mkdir(parents=True)
    (questions_dir / f"{task_id}.yaml").write_text(
        f"""
task_id: {task_id}
questions:
  - question_id: q1
    codebase_task_id: {task_id}
    intention_id: I-FR1
    question_type: capability_recognition
    prompt: Which behavior is implemented?
    options:
      A: alpha
      B: beta
      C: gamma
      D: delta
    correct_option: A
    source_trace:
      spec_section: functional_requirements
      source_requirement_id: FR1
      source_intention_id: I-FR1
""".lstrip(),
        encoding="utf-8",
    )


def test_export_tables_materializes_panels_and_metric_sources(tmp_path: Path) -> None:
    records_dir = tmp_path / "records"
    out_dir = tmp_path / "tables"
    questions_dir = tmp_path / "questions"
    artifact = tmp_path / "artifact"
    artifact.mkdir()
    (artifact / "src.rs").write_text("fn main() {}\n", encoding="utf-8")
    _write_question_bank(questions_dir)

    write_run(
        _build_run(
            "build-1",
            "scratch_minidb",
            "codex_gpt5_5_high",
            artifact,
            trial_id=7,
        ),
        records_dir,
    )
    write_run(
        _find_run(
            "find-1",
            "build-1",
            "scratch_minidb",
            "claude_code_opus_4_7_low",
            trial_id=3,
        ),
        records_dir,
    )

    metadata = export_analysis_tables(
        records_dir=records_dir,
        out_dir=out_dir,
        questions_dir=questions_dir,
    )

    manifest = json.loads((out_dir / "MANIFEST.json").read_text(encoding="utf-8"))
    assert manifest["schema_version"] == "2.3"
    assert manifest["generated_at"] == "2026-01-01T00:00:00+00:00"
    assert manifest["files"]["find_runs.csv"]["row_count"] == 1
    assert manifest["files"]["experiments.json"]["sha256"]

    assert metadata["panels"][PAPER_FINAL_PANEL_ID]["find_run_count"] == 1
    assert metadata["panels"][ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID]["find_run_count"] == 1
    assert metadata["panels"]["cross_effort_panel"]["find_run_count"] == 1

    find_row = load_find_run_table(out_dir, panel_id=PAPER_FINAL_PANEL_ID)[0]
    assert find_row["build_trial_id"] == 7
    assert find_row["finder_trial_id"] == 3
    assert find_row["n_correct"] == 1
    assert find_row["accuracy"] == 1.0
    assert find_row["retrieval_bytes"] == 700
    assert find_row["tool_call_bytes"] == 33
    assert find_row["total_bytes"] == 733
    assert find_row["metric_source"] == "proxy_novel_usage_events"
    assert find_row["session_log_retrieval_bytes"] == 111
    assert find_row["session_log_tool_call_bytes"] == 22
    assert find_row["session_log_total_bytes"] == 133
    assert ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID in find_row["panel_ids"].split("|")
    assert "cross_effort_panel" in find_row["panel_ids"].split("|")

    build_row = load_build_run_table(out_dir, panel_id="all_records")[0]
    assert build_row["build_run_id"] == "build-1"
    assert build_row["build_total_bytes"] == 11
    assert "scratch_all" in build_row["panel_ids"].split("|")

    question_row = load_question_answer_table(out_dir, panel_id=PAPER_FINAL_PANEL_ID)[0]
    assert question_row["question_type"] == "capability_recognition"
    assert question_row["source_requirement_id"] == "FR1"
    assert question_row["is_correct"] is True


def test_build_rows_keep_base_panel_without_find_runs(tmp_path: Path) -> None:
    records_dir = tmp_path / "records"
    out_dir = tmp_path / "tables"
    artifact = tmp_path / "artifact"
    artifact.mkdir()
    (artifact / "src.rs").write_text("fn main() {}\n", encoding="utf-8")

    write_run(
        _build_run("build-only", "scratch_nanoweb", "codex_gpt5_5_high", artifact),
        records_dir,
    )

    export_analysis_tables(records_dir=records_dir, out_dir=out_dir, questions_dir=None)

    build_rows = load_build_run_table(out_dir, panel_id="all_records")
    assert [row["build_run_id"] for row in build_rows] == ["build-only"]
    assert "scratch_all" in build_rows[0]["panel_ids"].split("|")


def test_control_records_load_into_control_panels_only(tmp_path: Path) -> None:
    records_dir = tmp_path / "records"
    out_dir = tmp_path / "tables"
    questions_dir = tmp_path / "questions"
    artifact = tmp_path / "artifact"
    artifact.mkdir()
    (artifact / "README.md").write_text("control artifact\n", encoding="utf-8")
    _write_question_bank(questions_dir)

    write_run(
        _build_run(
            "control-build-spec_only-scratch_minidb-t0-s0-test",
            "scratch_minidb",
            "control_spec_only",
            artifact,
        ),
        records_dir,
    )
    write_run(
        _find_run(
            "control-find-control-build-spec_only-codex_gpt5_5_high-t0-s0-test",
            "control-build-spec_only-scratch_minidb-t0-s0-test",
            "scratch_minidb",
            "codex_gpt5_5_high",
        ),
        records_dir,
    )

    metadata = export_analysis_tables(
        records_dir=records_dir,
        out_dir=out_dir,
        questions_dir=questions_dir,
    )

    assert metadata["panels"][CONTROL_PANEL_ID]["find_run_count"] == 1
    assert metadata["panels"]["spec_only"]["find_run_count"] == 1
    assert PAPER_FINAL_PANEL_ID not in metadata["panels"]

    find_row = load_find_run_table(out_dir, panel_id=CONTROL_PANEL_ID)[0]
    panel_ids = find_row["panel_ids"].split("|")
    assert CONTROL_PANEL_ID in panel_ids
    assert "spec_only" in panel_ids
    assert PAPER_FINAL_PANEL_ID not in panel_ids


def test_non_compiling_builds_are_artifact_present_diagnostics_not_primary(
    tmp_path: Path,
) -> None:
    records_dir = tmp_path / "records"
    out_dir = tmp_path / "tables"
    questions_dir = tmp_path / "questions"
    artifact = tmp_path / "artifact"
    artifact.mkdir()
    (artifact / "src.rs").write_text("fn main() {}\n", encoding="utf-8")
    _write_question_bank(questions_dir)

    write_run(
        _build_run(
            "build-compile-fail",
            "scratch_minidb",
            "codex_gpt5_5_high",
            artifact,
            compile_pass=False,
        ),
        records_dir,
    )
    write_run(
        _find_run(
            "find-compile-fail",
            "build-compile-fail",
            "scratch_minidb",
            "claude_code_opus_4_7_high",
        ),
        records_dir,
    )

    metadata = export_analysis_tables(
        records_dir=records_dir,
        out_dir=out_dir,
        questions_dir=questions_dir,
    )

    assert PAPER_FINAL_PANEL_ID not in metadata["panels"]
    assert metadata["panels"][ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID]["find_run_count"] == 1
    assert BUILD_FIDELITY_DIAGNOSTIC_PANEL_ID not in metadata["panels"]
    find_row = load_find_run_table(
        out_dir,
        panel_id=ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID,
    )[0]
    assert find_row["compile_pass"] is False
    panel_ids = find_row["panel_ids"].split("|")
    assert ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID in panel_ids
    assert PAPER_FINAL_PANEL_ID not in panel_ids


def test_missing_artifacts_are_diagnostics_not_primary_panel(tmp_path: Path) -> None:
    records_dir = tmp_path / "records"
    out_dir = tmp_path / "tables"
    questions_dir = tmp_path / "questions"
    artifact = tmp_path / "missing-artifact"
    _write_question_bank(questions_dir)

    write_run(
        _build_run(
            "build-missing-artifact",
            "scratch_minidb",
            "codex_gpt5_5_high",
            artifact,
            compile_pass=True,
        ),
        records_dir,
    )
    write_run(
        _find_run(
            "find-missing-artifact",
            "build-missing-artifact",
            "scratch_minidb",
            "claude_code_opus_4_7_high",
        ),
        records_dir,
    )

    metadata = export_analysis_tables(
        records_dir=records_dir,
        out_dir=out_dir,
        questions_dir=questions_dir,
    )

    assert PAPER_FINAL_PANEL_ID not in metadata["panels"]
    assert ARTIFACT_PRESENT_DIAGNOSTIC_PANEL_ID not in metadata["panels"]
    assert metadata["panels"][BUILD_FIDELITY_DIAGNOSTIC_PANEL_ID]["find_run_count"] == 1
    find_row = load_find_run_table(out_dir, panel_id=BUILD_FIDELITY_DIAGNOSTIC_PANEL_ID)[0]
    assert find_row["compile_pass"] is True
    assert PAPER_FINAL_PANEL_ID not in find_row["panel_ids"].split("|")


def test_export_rejects_missing_question_bank_when_questions_dir_is_supplied(
    tmp_path: Path,
) -> None:
    records_dir = tmp_path / "records"
    out_dir = tmp_path / "tables"
    questions_dir = tmp_path / "questions"
    artifact = tmp_path / "artifact"
    questions_dir.mkdir()
    artifact.mkdir()
    (artifact / "src.rs").write_text("fn main() {}\n", encoding="utf-8")

    write_run(
        _build_run("build-1", "scratch_minidb", "codex_gpt5_5_high", artifact),
        records_dir,
    )
    write_run(
        _find_run("find-1", "build-1", "scratch_minidb", "codex_gpt5_5_high"),
        records_dir,
    )

    with pytest.raises(ValueError, match="no question bank"):
        export_analysis_tables(
            records_dir=records_dir,
            out_dir=out_dir,
            questions_dir=questions_dir,
        )
