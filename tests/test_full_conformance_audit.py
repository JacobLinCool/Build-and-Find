from __future__ import annotations

import csv
import importlib.util
import json
import math
import re
import shlex
import sys
from collections import Counter
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
AUDIT_DIR = ROOT / "rebuttal" / "full_conformance_audit"
# Kept in the authors' records; the public release has only author_decisions.csv.
AUTHOR_WORKSHEET = AUDIT_DIR / "author_adjudication_worksheet.md"


def load_script_module(name: str, relative_path: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative_path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


audit = load_script_module(
    "run_full_conformance_audit",
    "scripts/run_full_conformance_audit.py",
)
sensitivity = load_script_module(
    "analyze_rebuttal_label_sensitivity",
    "scripts/analyze_rebuttal_label_sensitivity.py",
)


def test_frozen_population_and_stratified_sample_are_exact_and_deterministic() -> None:
    pairs = audit.load_population()
    assert len(pairs) == 592
    assert len({pair.artifact_key for pair in pairs}) == 41

    first_keys, first_allocation = audit.stratified_confirmed_sample(pairs)
    second_keys, second_allocation = audit.stratified_confirmed_sample(pairs)

    assert first_keys == second_keys
    assert first_allocation == second_allocation
    assert len(first_keys) == math.ceil(0.10 * len(pairs))
    assert sum(first_allocation.values()) == len(first_keys)
    assert set(first_allocation) == {
        ("scratch_minidb", "claude"),
        ("scratch_minidb", "codex"),
        ("scratch_minidb", "mimo"),
        ("scratch_nanoweb", "claude"),
        ("scratch_nanoweb", "codex"),
        ("scratch_nanoweb", "mimo"),
    }


def test_frozen_crosscheck_manifest_must_equal_recomputed_protocol(tmp_path: Path) -> None:
    pairs = audit.load_population()
    primary = {pair.key: {"row": {"verdict": "confirmed_gold"}} for pair in pairs}
    rows, sampling = audit.derive_crosscheck_selection(pairs, primary)
    rendered = audit.render_csv(rows, audit.CROSSCHECK_SELECTION_FIELDS)
    sampling["selection_hash"] = audit.sha256_bytes(rendered.encode("utf-8"))
    (tmp_path / "crosscheck_sampling_manifest.json").write_text(
        json.dumps(sampling),
        encoding="utf-8",
    )

    audit.validate_crosscheck_selection(
        output_dir=tmp_path,
        pairs=pairs,
        primary_by_key=primary,
        selection=rows,
    )
    with pytest.raises(RuntimeError, match="differs from recomputed protocol"):
        audit.validate_crosscheck_selection(
            output_dir=tmp_path,
            pairs=pairs,
            primary_by_key=primary,
            selection=rows[:-1],
        )


def test_known_regression_requires_the_expected_mapped_label() -> None:
    assert audit.verdict_reproduces_label("non_gold", "implemented_non_gold")
    assert not audit.verdict_reproduces_label("ambiguous", "implemented_non_gold")
    assert audit.verdict_reproduces_label("ambiguous", "absent_or_ambiguous")
    assert audit.verdict_reproduces_label("absent", "absent_or_ambiguous")


def test_rendered_primary_prompt_omits_existing_label_and_prior_rationale() -> None:
    pairs = audit.load_population()
    first_artifact = pairs[0].artifact_key
    artifact_pairs = [pair for pair in pairs if pair.artifact_key == first_artifact]
    prompt = audit.render_prompt(audit.PROMPT_TEMPLATE_PATH, artifact_pairs)

    assert "consensus_prelabel" not in prompt
    assert "Stable selected-finder consensus prelabel" not in prompt
    assert "model_vote_summary" not in prompt
    assert "source_audit_label" not in prompt
    assert first_artifact[1] in prompt
    rendered_ids = set(re.findall(r'"question_id": "(q_[^"]+)"', prompt))
    assert rendered_ids == {pair.source["question_id"] for pair in artifact_pairs}


def test_payload_validation_normalizes_and_checks_real_spans(tmp_path: Path) -> None:
    source_dir = tmp_path / "src"
    source_dir.mkdir()
    source_path = source_dir / "main.rs"
    source_path.write_text("fn main() {\n    run();\n}\n", encoding="utf-8")
    pair = audit.Pair(
        source={
            "task_id": "scratch_minidb",
            "artifact_id": "build-test",
            "question_id": "q_db_test",
        },
        question={"correct_option": "B"},
    )
    payload = {
        "task_id": "scratch_minidb",
        "artifact_id": "build-test",
        "rows": [
            {
                "question_id": "q_db_test",
                "verdict": "confirmed_gold",
                "actual_option": "B",
                "confidence": "high",
                "runtime_reachability_checked": True,
                "evidence_files": ["./src/main.rs"],
                "evidence_symbols": ["main", "run"],
                "evidence_spans": [{"file": "src/main.rs", "start_line": 1, "end_line": 3}],
                "rationale": "main dispatches directly to run on the live path.",
            }
        ],
    }

    normalized = audit.validate_payload(
        payload=payload,
        pairs=[pair],
        artifact_root=tmp_path,
    )

    assert normalized["rows"][0]["evidence_files"] == ["src/main.rs"]
    assert normalized["rows"][0]["evidence_spans"][0]["end_line"] == 3


def test_payload_validation_rejects_out_of_range_span(tmp_path: Path) -> None:
    path = tmp_path / "main.rs"
    path.write_text("fn main() {}\n", encoding="utf-8")
    pair = audit.Pair(
        source={
            "task_id": "scratch_minidb",
            "artifact_id": "build-test",
            "question_id": "q_db_test",
        },
        question={"correct_option": "B"},
    )
    payload = {
        "task_id": "scratch_minidb",
        "artifact_id": "build-test",
        "rows": [
            {
                "question_id": "q_db_test",
                "verdict": "confirmed_gold",
                "actual_option": "B",
                "confidence": "high",
                "runtime_reachability_checked": True,
                "evidence_files": ["main.rs"],
                "evidence_symbols": ["main"],
                "evidence_spans": [{"file": "main.rs", "start_line": 1, "end_line": 2}],
                "rationale": "main is the live entry point.",
            }
        ],
    }

    with pytest.raises(RuntimeError, match="exceeds 1 lines"):
        audit.validate_payload(
            payload=payload,
            pairs=[pair],
            artifact_root=tmp_path,
        )


def test_full_audit_correction_loader_recomputes_basis_and_label() -> None:
    row = {field: "" for field in audit.DISAGREEMENT_FIELDS}
    row.update(
        {
            "task_id": "scratch_minidb",
            "artifact_id": "build-test",
            "question_id": "q_db_01",
            "consensus_label": "implemented_gold",
            "primary_verdict": "non_gold",
            "primary_confidence": "high",
            "primary_evidence_files": "src/main.rs",
            "primary_evidence_symbols": "main",
            "primary_evidence_spans": "src/main.rs:1-3",
            "primary_rationale": "The live path uniquely implements option A.",
            "primary_execution_engine": "collaboration.spawn_agent",
            "primary_model_id": "gpt-5.6-sol",
            "primary_reasoning_effort": "ultra",
            "primary_prompt_version": audit.PROMPT_VERSION,
            "primary_prompt_template_sha256": "a" * 64,
            "primary_rendered_prompt_sha256": "b" * 64,
            "primary_session_id": "full_audit_p01",
            "primary_session_result_sha256": "c" * 64,
            "primary_session_result_hash_source": ("completion_record_and_parent_verified"),
            "primary_integrity_attestation_sha256": "f" * 64,
            "crosscheck_verdict": "non_gold",
            "crosscheck_confidence": "medium",
            "crosscheck_evidence_files": "src/main.rs",
            "crosscheck_evidence_symbols": "main",
            "crosscheck_evidence_spans": "src/main.rs:1-3",
            "crosscheck_rationale": "The independent live trace also reaches A.",
            "crosscheck_execution_engine": "collaboration.spawn_agent",
            "crosscheck_model_id": "gpt-5.6-terra",
            "crosscheck_reasoning_effort": "ultra",
            "crosscheck_prompt_version": audit.PROMPT_VERSION,
            "crosscheck_prompt_template_sha256": "a" * 64,
            "crosscheck_rendered_prompt_sha256": "d" * 64,
            "crosscheck_session_id": "full_audit_c01",
            "crosscheck_session_result_sha256": "e" * 64,
            "crosscheck_session_result_hash_source": ("post_hoc_parent_attestation"),
            "crosscheck_integrity_attestation_sha256": "9" * 64,
            "crosscheck_agrees_with_primary": "true",
            "disagreement_basis": "both_passes",
            "flag_verdicts": "non_gold|non_gold",
            "proposed_sensitivity_label": "implemented_non_gold",
            "author_adjudication_status": "pending",
        }
    )

    corrections = sensitivity._load_full_audit_corrections([row])

    assert len(corrections) == 1
    assert corrections[0].new_label == "implemented_non_gold"
    assert corrections[0].confidence == "medium"


def test_correction_universe_preserves_prior_and_full_audit_provenance() -> None:
    prior = sensitivity.Correction(
        task_id="scratch_minidb",
        artifact_id="build-test",
        question_id="q_db_01",
        old_label="implemented_gold",
        new_label="implemented_non_gold",
        reason="prior reason",
        provenance="pilot_source_audit",
        confidence="source_confirmed",
    )
    full = sensitivity.Correction(
        task_id=prior.task_id,
        artifact_id=prior.artifact_id,
        question_id=prior.question_id,
        old_label=prior.old_label,
        new_label=prior.new_label,
        reason="full reason",
        provenance="full_conformance_audit_both_passes",
        confidence="high",
    )

    merged = sensitivity._merge_correction_universe(
        observed=[prior],
        full_audit=[full],
    )

    assert len(merged) == 1
    assert merged[0].provenance == ("pilot_source_audit|full_conformance_audit_both_passes")
    assert "Prior audit: prior reason" in merged[0].reason
    assert "Full audit: full reason" in merged[0].reason


def test_sensitivity_bundle_rejects_truncated_disagreement_union(tmp_path: Path) -> None:
    minidb_path = tmp_path / "minidb_adjudication.csv"
    nanoweb_path = tmp_path / "nanoweb_adjudication.csv"
    disagreement_path = tmp_path / "disagreements.csv"
    validation_path = tmp_path / "validation_manifest.json"
    output_manifest_path = tmp_path / "output_manifest.json"
    canonical = [
        {
            "task_id": "scratch_minidb" if index < 302 else "scratch_nanoweb",
            "artifact_id": f"build-{index // 15:03d}",
            "question_id": f"q_{index:03d}",
            "flagged_for_author": "true" if index in {0, 1} else "false",
        }
        for index in range(592)
    ]
    audit.write_csv(
        minidb_path,
        canonical[:302],
        ("task_id", "artifact_id", "question_id", "flagged_for_author"),
    )
    audit.write_csv(
        nanoweb_path,
        canonical[302:],
        ("task_id", "artifact_id", "question_id", "flagged_for_author"),
    )
    truncated = [canonical[0]]
    audit.write_csv(
        disagreement_path,
        truncated,
        ("task_id", "artifact_id", "question_id", "flagged_for_author"),
    )
    validation_path.write_text(
        json.dumps({"population_pairs": 592, "author_flag_count": 2}),
        encoding="utf-8",
    )
    tracked = (minidb_path, nanoweb_path, disagreement_path, validation_path)
    output_manifest_path.write_text(
        json.dumps(
            {
                "output_sha256": {
                    sensitivity._display_path(path): sensitivity._sha256(path) for path in tracked
                }
            }
        ),
        encoding="utf-8",
    )
    inputs = {
        "full_audit_minidb_adjudication": minidb_path,
        "full_audit_nanoweb_adjudication": nanoweb_path,
        "full_audit_disagreements": disagreement_path,
        "full_audit_validation_manifest": validation_path,
        "full_audit_output_manifest": output_manifest_path,
    }

    with pytest.raises(
        RuntimeError,
        match="differs from canonical flagged rows.*--author-decisions",
    ):
        sensitivity._validate_full_audit_bundle(
            inputs=inputs,
            disagreement_rows=truncated,
        )


def _author_decision_row(**overrides: str) -> dict[str, str]:
    row = {
        "task_id": "scratch_minidb",
        "artifact_id": "build-test",
        "question_id": "q_db_01",
        "flag_basis": "primary_only",
        "author_decision": "absent_or_ambiguous",
        "decided_by": "author",
        "decided_on": "2026-07-31",
        "worksheet_item": "#1",
        "rationale": "not recorded at decision time",
    }
    row.update(overrides)
    return row


def test_author_decision_loader_requires_flagged_pair_and_matching_basis() -> None:
    flags = [
        {
            "task_id": "scratch_minidb",
            "artifact_id": "build-test",
            "question_id": "q_db_01",
            "disagreement_basis": "primary_pass",
        }
    ]

    decisions = sensitivity._load_author_decisions(
        [_author_decision_row()],
        disagreement_rows=flags,
    )

    assert [decision.key for decision in decisions] == [("scratch_minidb", "build-test", "q_db_01")]
    invalid_rows = {
        "flag basis mismatch": [_author_decision_row(flag_basis="both_passes")],
        "does not match a full-audit flag": [_author_decision_row(question_id="q_db_02")],
        "invalid author decision": [_author_decision_row(author_decision="implemented_gold")],
        "invalid decided_on": [_author_decision_row(decided_on="20260731")],
        "duplicate author decision": [
            _author_decision_row(),
            _author_decision_row(worksheet_item="#2"),
        ],
    }
    for message, rows in invalid_rows.items():
        with pytest.raises(RuntimeError, match=message):
            sensitivity._load_author_decisions(rows, disagreement_rows=flags)


def _flag_correction(question_id: str, *, provenance: str) -> sensitivity.Correction:
    return sensitivity.Correction(
        task_id="scratch_minidb",
        artifact_id="build-test",
        question_id=question_id,
        old_label="implemented_gold",
        new_label="absent_or_ambiguous",
        reason=f"reason {question_id}",
        provenance=provenance,
        confidence="high",
    )


def test_author_adjudicated_scenarios_keep_gold_and_add_unreviewed_flags() -> None:
    observed = [
        _flag_correction("q_db_01", provenance="spot_audit_sample_01"),
        _flag_correction("q_db_04", provenance="spot_audit_sample_02"),
    ]
    full_audit = [
        _flag_correction(f"q_db_0{index}", provenance="full_conformance_audit_both_passes")
        for index in range(1, 6)
    ]
    decisions = [
        sensitivity.AuthorDecision(**_author_decision_row(question_id="q_db_01")),
        sensitivity.AuthorDecision(
            **_author_decision_row(
                question_id="q_db_02",
                author_decision="non_gold",
                worksheet_item="#2",
            )
        ),
        sensitivity.AuthorDecision(
            **_author_decision_row(
                question_id="q_db_03",
                author_decision="keep_implemented_gold",
                worksheet_item="#3",
            )
        ),
    ]

    adjudicated, worst_case = sensitivity._author_adjudicated_corrections(
        observed=observed,
        full_audit=full_audit,
        decisions=decisions,
    )

    adjudicated_labels = {row.question_id: row.new_label for row in adjudicated}
    worst_case_labels = {row.question_id: row.new_label for row in worst_case}
    assert adjudicated_labels == {
        "q_db_01": "absent_or_ambiguous",
        "q_db_02": "implemented_non_gold",
        "q_db_04": "absent_or_ambiguous",
    }
    assert worst_case_labels == {**adjudicated_labels, "q_db_05": "absent_or_ambiguous"}
    by_question = {row.question_id: row for row in adjudicated}
    assert by_question["q_db_01"].provenance == (
        "spot_audit_sample_01|full_conformance_audit_both_passes|author_adjudication"
    )
    assert by_question["q_db_02"].confidence == "author_confirmed"
    worst_by_question = {row.question_id: row for row in worst_case}
    assert worst_by_question["q_db_04"].provenance == (
        "spot_audit_sample_02|full_conformance_audit_both_passes"
    )

    with pytest.raises(RuntimeError, match="kept implemented_gold on observed"):
        sensitivity._author_adjudicated_corrections(
            observed=observed,
            full_audit=full_audit,
            decisions=[
                sensitivity.AuthorDecision(
                    **_author_decision_row(
                        question_id="q_db_04",
                        author_decision="keep_implemented_gold",
                    )
                )
            ],
        )


@pytest.mark.skipif(
    not AUTHOR_WORKSHEET.is_file(),
    reason="author adjudication worksheet absent (not part of the public release)",
)
def test_author_decisions_csv_transcribes_worksheet() -> None:
    audit_dir = AUDIT_DIR
    worksheet = AUTHOR_WORKSHEET.read_text(encoding="utf-8")
    entries = re.findall(
        r"^## (\d+)\. (scratch_\w+) \| (\w+) \| ([0-9a-f]{8}) \| (q_\w+) .*?"
        r"^- AUTHOR DECISION: \[(\w+)\]",
        worksheet,
        flags=re.S | re.M,
    )
    with (audit_dir / "author_decisions.csv").open(newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))
    with (audit_dir / "disagreements.csv").open(newline="", encoding="utf-8") as handle:
        flags = list(csv.DictReader(handle))

    decisions = sensitivity._load_author_decisions(rows, disagreement_rows=flags)

    assert len(entries) == len(decisions) == 43
    assert [
        (
            decision.worksheet_item,
            decision.task_id,
            decision.artifact_id.split("-")[2],
            decision.artifact_id[-8:],
            decision.question_id,
            decision.author_decision,
        )
        for decision in decisions
    ] == [(f"#{number}", *rest) for number, *rest in entries]
    assert Counter(decision.author_decision for decision in decisions) == Counter(
        {"absent_or_ambiguous": 39, "non_gold": 2, "keep_implemented_gold": 2}
    )
    assert [
        decision.worksheet_item
        for decision in decisions
        if decision.author_decision == "keep_implemented_gold"
    ] == ["#7", "#11"]
    assert Counter(decision.artifact_id.split("-")[2] for decision in decisions) == Counter(
        {"codex_gpt5_4_mini_low": 25, "codex_gpt5_5_low": 18}
    )
    assert {
        (decision.decided_by, decision.decided_on, decision.rationale) for decision in decisions
    } == {("author", "2026-07-31", "not recorded at decision time")}


def test_adjudicated_sensitivity_manifest_records_actual_inputs_and_command() -> None:
    audit_dir = ROOT / "rebuttal" / "full_conformance_audit"
    output_dir = audit_dir / "label_sensitivity_adjudicated"
    manifest = json.loads((output_dir / "manifest.json").read_text(encoding="utf-8"))

    assert manifest["scenario_set"] == "fully_adjudicated"
    assert manifest["analysis_script_sha256"] == sensitivity._sha256(
        ROOT / manifest["analysis_script"]
    )
    command = shlex.split(manifest["analysis_command"])
    assert command[command.index("--author-decisions") + 1] == (
        "rebuttal/full_conformance_audit/author_decisions.csv"
    )
    assert command[command.index("--full-audit-disagreements") + 1] == (
        "rebuttal/full_conformance_audit/disagreements.csv"
    )
    assert command[command.index("--output-dir") + 1] == (
        "rebuttal/full_conformance_audit/label_sensitivity_adjudicated"
    )
    assert manifest["input_paths"]["author_decisions"] == (
        "rebuttal/full_conformance_audit/author_decisions.csv"
    )
    model_path = "rebuttal/full_conformance_audit/model_assisted_decisions.csv"
    assert command[command.index("--model-assisted-decisions") + 1] == model_path
    assert manifest["input_paths"]["model_assisted_decisions"] == model_path
    corrections_path = (
        "runs_release/reports/artifact_conformance_consensus/final/label_corrections_v1_1.csv"
    )
    assert command[command.index("--label-corrections") + 1] == corrections_path
    assert manifest["input_paths"]["label_corrections"] == corrections_path
    for name, path in manifest["input_paths"].items():
        assert manifest["input_sha256"][name] == sensitivity._sha256(ROOT / path), name
    for path, digest in manifest["output_sha256"].items():
        assert sensitivity._sha256(ROOT / path) == digest, path
    assert manifest["scenario_correction_counts"] == {
        "baseline": 0,
        "all_observed_corrected": 9,
        "author_adjudicated": 48,
        "fully_adjudicated": 195,
        "author_adjudicated_plus_unreviewed_flags": 203,
    }
    assert manifest["scenario_order"] == list(sensitivity.FULLY_ADJUDICATED_SCENARIOS)
    author = manifest["author_adjudication"]
    assert (
        author["full_audit_flags"],
        author["reviewed_flags"],
        author["confirmed_relabels"],
        author["kept_implemented_gold"],
        author["unreviewed_flags"],
    ) == (205, 43, 41, 2, 162)
    model = manifest["model_assisted_adjudication"]
    assert (
        model["decision_rows"],
        model["model_relabels"],
        model["kept_implemented_gold"],
        model["observed_in_model_relabels"],
        model["fully_adjudicated_pairs"],
    ) == (162, 154, 8, 7, 195)
    assert model["review_status"] == ["author_approved"]
    assert model["reviewed_on"] == ["2026-10-16"]


def test_adjudicated_sensitivity_rb_orders_match_reported_claims() -> None:
    output_dir = ROOT / "rebuttal" / "full_conformance_audit" / "label_sensitivity_adjudicated"
    with (output_dir / "rb_rankings.csv").open(newline="", encoding="utf-8") as handle:
        orders = sensitivity._builder_orders(list(csv.DictReader(handle)))

    baseline_low = orders[("baseline", "low_effort")]
    adjudicated_low = orders[("author_adjudicated", "low_effort")]
    for scenario in (
        "author_adjudicated",
        "fully_adjudicated",
        "author_adjudicated_plus_unreviewed_flags",
    ):
        assert orders[(scenario, "high_effort")] == orders[("baseline", "high_effort")]
    assert baseline_low[:2] == ["codex_gpt5_5_low", "codex_gpt5_4_mini_low"]
    assert adjudicated_low == [baseline_low[1], baseline_low[0], *baseline_low[2:]]
    assert orders[("fully_adjudicated", "low_effort")] == adjudicated_low
    assert orders[("author_adjudicated_plus_unreviewed_flags", "low_effort")] == adjudicated_low


def test_evidence_audit_overlap_counts_sampled_answers_on_relabeled_pairs() -> None:
    output_dir = ROOT / "rebuttal" / "full_conformance_audit" / "label_sensitivity_adjudicated"
    with (output_dir / "evidence_audit_overlap.csv").open(newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))
    counts = {
        (row["scenario"], row["slice_id"], row["audit_label"]): (
            int(row["on_relabeled_pairs"]),
            int(row["sampled_answers"]),
        )
        for row in rows
    }
    # Paper Appendix I: supported answers on pairs relabeled under the adjudicated labels.
    assert counts[("fully_adjudicated", "all", "Supported")] == (23, 65)
    assert counts[("fully_adjudicated", "low_prior", "Supported")] == (10, 35)
    assert all(on == 0 for (scenario, _, _), (on, _) in counts.items() if scenario == "baseline")
    assert (
        sum(
            on
            for (scenario, slice_id, _), (on, _) in counts.items()
            if scenario == "author_adjudicated_plus_unreviewed_flags" and slice_id == "all"
        )
        == 26
    )

    sample = [
        {
            "find_run_id": f"find-{index}",
            "task_id": "scratch_minidb",
            "build_run_id": "build-test",
            "question_id": question_id,
            "audit_label": label,
            "prior_status": prior,
        }
        for index, (question_id, label, prior) in enumerate(
            (
                ("q_db_01", "Supported", "low_prior"),
                ("q_db_02", "Supported", "high_prior"),
                ("q_db_01", "Prior-like", "low_prior"),
            )
        )
    ]
    overlap = sensitivity._evidence_audit_overlap_rows(
        sample_rows=sample,
        scenario_corrections={
            "baseline": (),
            "relabel": (
                _flag_correction("q_db_01", provenance="full_conformance_audit_both_passes"),
            ),
        },
    )
    by_key = {
        (row["scenario"], row["slice_id"], row["audit_label"]): (
            row["on_relabeled_pairs"],
            row["sampled_answers"],
        )
        for row in overlap
    }
    assert by_key[("relabel", "all", "Supported")] == (1, 2)
    assert by_key[("relabel", "low_prior", "Prior-like")] == (1, 1)
    assert by_key[("baseline", "all", "Supported")] == (0, 2)
    with pytest.raises(RuntimeError, match="unknown evidence-audit label"):
        sensitivity._evidence_audit_overlap_rows(
            sample_rows=[{**sample[0], "audit_label": "Partial"}],
            scenario_corrections={"baseline": ()},
        )
    with pytest.raises(RuntimeError, match="duplicate find-run answers"):
        sensitivity._evidence_audit_overlap_rows(
            sample_rows=[sample[0], sample[0]],
            scenario_corrections={"baseline": ()},
        )


def test_relabeled_pair_tables_match_appendix_h_counts() -> None:
    output_dir = ROOT / "rebuttal" / "full_conformance_audit" / "label_sensitivity_adjudicated"
    with (output_dir / "relabeled_pairs.csv").open(newline="", encoding="utf-8") as handle:
        relabeled = {(row["scenario"], row["new_label"]): row for row in csv.DictReader(handle)}
    # Paper Appendix H: 195 relabeled pairs (190 absent/ambiguous, 5 non-gold);
    # finders chose the gold option in 6,829 of their 7,020 answer rows.
    fully = relabeled[("fully_adjudicated", "all")]
    assert (fully["relabeled_pairs"], fully["primary_answer_rows"], fully["gold_option_rows"]) == (
        "195",
        "7020",
        "6829",
    )
    assert relabeled[("fully_adjudicated", "absent_or_ambiguous")]["relabeled_pairs"] == "190"
    assert relabeled[("fully_adjudicated", "implemented_non_gold")]["relabeled_pairs"] == "5"

    with (output_dir / "question_labels.csv").open(newline="", encoding="utf-8") as handle:
        questions = [
            row for row in csv.DictReader(handle) if row["scenario"] == "fully_adjudicated"
        ]
    top = sorted(questions, key=lambda row: -int(row["relabeled_pairs"]))[:3]
    # Three nanoweb questions hold 59 of the 195 relabels and stay implemented-gold
    # for at most one of the 20 compile-pass artifacts.
    assert {row["question_id"] for row in top} == {"q_nw_02", "q_nw_11", "q_nw_14"}
    assert sum(int(row["relabeled_pairs"]) for row in top) == 59
    assert {row["compile_pass_artifacts"] for row in top} == {"20"}
    assert max(int(row["scenario_implemented_gold"]) for row in top) == 1
    assert sum(int(row["relabeled_pairs"]) for row in questions) == 195


def test_relabeled_pair_rows_count_pairs_leaving_implemented_gold() -> None:
    def label(artifact_id: str, question_id: str, final_label: str, compile_pass: str = "true"):
        return {
            "task_id": "scratch_nanoweb",
            "artifact_id": artifact_id,
            "question_id": question_id,
            "final_label": final_label,
            "compile_pass": compile_pass,
        }

    baseline = [
        label("a1", "q_nw_02", "implemented_gold"),
        label("a2", "q_nw_02", "implemented_gold"),
        label("a3", "q_nw_02", "implemented_gold", compile_pass="false"),
        label("a1", "q_nw_04", "implemented_gold"),
    ]
    relabeled = [label("a1", "q_nw_02", "absent_or_ambiguous"), *baseline[1:]]
    rows = sensitivity._question_label_rows(
        scenario_labels={"baseline": baseline, "relabel": relabeled}
    )
    by_key = {(row["scenario"], row["question_id"]): row for row in rows}
    assert by_key[("relabel", "q_nw_02")]["compile_pass_artifacts"] == 2
    assert by_key[("relabel", "q_nw_02")]["scenario_implemented_gold"] == 1
    assert by_key[("relabel", "q_nw_02")]["relabeled_pairs"] == 1
    assert by_key[("relabel", "q_nw_04")]["relabeled_pairs"] == 0
    assert by_key[("baseline", "q_nw_02")]["relabeled_pairs"] == 0
    with pytest.raises(RuntimeError, match="adds implemented_gold"):
        sensitivity._question_label_rows(
            scenario_labels={"baseline": relabeled, "relabel": baseline}
        )

    evidence = [
        {
            "new_label": "absent_or_ambiguous",
            "included_scenarios": "relabel|worst",
            "primary_answer_rows": 36,
            "primary_correct_answer_rows": 30,
        },
        {
            "new_label": "implemented_non_gold",
            "included_scenarios": "worst",
            "primary_answer_rows": 36,
            "primary_correct_answer_rows": 36,
        },
    ]
    counts = {
        (row["scenario"], row["new_label"]): (
            row["relabeled_pairs"],
            row["primary_answer_rows"],
            row["gold_option_rows"],
        )
        for row in sensitivity._relabeled_pair_rows(
            scenarios=("baseline", "relabel", "worst"),
            correction_evidence=evidence,
        )
    }
    assert counts == {
        ("relabel", "all"): (1, 36, 30),
        ("relabel", "absent_or_ambiguous"): (1, 36, 30),
        ("worst", "all"): (2, 72, 66),
        ("worst", "absent_or_ambiguous"): (1, 36, 30),
        ("worst", "implemented_non_gold"): (1, 36, 36),
    }
    assert sensitivity._headline_scenario(sensitivity.FULLY_ADJUDICATED_SCENARIOS) == (
        "fully_adjudicated"
    )
    assert sensitivity._headline_scenario(sensitivity.AUTHOR_SCENARIOS) == "author_adjudicated"


def test_existing_adjudicated_scenarios_do_not_depend_on_model_assisted_decisions() -> None:
    output_dir = ROOT / "rebuttal" / "full_conformance_audit" / "label_sensitivity_adjudicated"
    with (output_dir / "rb_rankings.csv").open(newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))
    scores = {
        (row["scenario"], row["panel_id"], row["builder_id"]): float(row["score"]) for row in rows
    }
    # The author-adjudicated scenario must be unaffected by the added
    # fully_adjudicated scenario.
    assert scores[("author_adjudicated", "low_effort", "codex_gpt5_4_mini_low")] == pytest.approx(
        1.128049, abs=5e-7
    )
    assert scores[("author_adjudicated", "low_effort", "codex_gpt5_5_low")] == pytest.approx(
        1.168112, abs=5e-7
    )


def _model_decision_row(**overrides: str) -> dict[str, str]:
    row = {
        "task_id": "scratch_minidb",
        "artifact_id": "build-test",
        "question_id": "q_db_02",
        "flag_basis": "both_passes",
        "decision": "absent_or_ambiguous",
        "actual_option": "none",
        "evidence": "src/main.rs:1-10",
        "rationale": "gold holds only partially on the live path",
        "confidence": "high",
        "pass_agreement": "agree",
        "decided_by": "model-assisted adjudication (Claude), two passes with arbitration",
        "review_status": "author_approved",
        "decided_on": "2026-10-16",
        "reviewed_on": "2026-10-16",
    }
    row.update(overrides)
    return row


def test_model_assisted_decision_loader_requires_exactly_the_unreviewed_flags() -> None:
    flags = [
        {
            "task_id": "scratch_minidb",
            "artifact_id": "build-test",
            "question_id": question_id,
            "disagreement_basis": basis,
        }
        for question_id, basis in (("q_db_01", "primary_pass"), ("q_db_02", "both_passes"))
    ]
    author = sensitivity._load_author_decisions(
        [_author_decision_row()],
        disagreement_rows=flags,
    )

    decisions = sensitivity._load_model_assisted_decisions(
        [_model_decision_row()],
        disagreement_rows=flags,
        author_decisions=author,
    )

    assert [decision.key for decision in decisions] == [("scratch_minidb", "build-test", "q_db_02")]
    pending = sensitivity._load_model_assisted_decisions(
        [_model_decision_row(review_status="pending_author_review", reviewed_on="")],
        disagreement_rows=flags,
        author_decisions=author,
    )
    assert pending[0].review == "pending_author_review"
    invalid_rows = {
        "overlaps an author decision": [
            _model_decision_row(),
            _model_decision_row(question_id="q_db_01", flag_basis="primary_only"),
        ],
        "flag basis mismatch": [_model_decision_row(flag_basis="primary_only")],
        "does not match a full-audit flag": [_model_decision_row(question_id="q_db_03")],
        "invalid model-assisted decision": [_model_decision_row(decision="implemented_gold")],
        "invalid actual_option": [_model_decision_row(actual_option="E")],
        "invalid pass_agreement": [_model_decision_row(pass_agreement="single")],
        "lacks review_status": [_model_decision_row(review_status=" ")],
        "invalid review_status": [_model_decision_row(review_status="author_reviewed")],
        "invalid reviewed_on": [_model_decision_row(reviewed_on="")],
        "reviewed_on precedes decided_on": [_model_decision_row(reviewed_on="2026-10-15")],
        "pending model-assisted decision": [
            _model_decision_row(review_status="pending_author_review")
        ],
        "duplicate model-assisted decision": [_model_decision_row(), _model_decision_row()],
    }
    for message, rows in invalid_rows.items():
        with pytest.raises(RuntimeError, match=message):
            sensitivity._load_model_assisted_decisions(
                rows, disagreement_rows=flags, author_decisions=author
            )
    with pytest.raises(RuntimeError, match="must cover every flag"):
        sensitivity._load_model_assisted_decisions(
            [_model_decision_row()],
            disagreement_rows=[
                *flags,
                {**flags[1], "question_id": "q_db_03"},
            ],
            author_decisions=author,
        )


def test_fully_adjudicated_corrections_add_model_relabels_and_keep_gold() -> None:
    observed = [_flag_correction("q_db_04", provenance="spot_audit_sample_02")]
    full_audit = [
        _flag_correction(f"q_db_0{index}", provenance="full_conformance_audit_both_passes")
        for index in range(1, 6)
    ]
    author_decisions = [sensitivity.AuthorDecision(**_author_decision_row(question_id="q_db_01"))]
    author_adjudicated, worst_case = sensitivity._author_adjudicated_corrections(
        observed=observed,
        full_audit=full_audit,
        decisions=author_decisions,
    )
    model_decisions = [
        sensitivity.ModelAssistedDecision(
            **_model_decision_row(question_id="q_db_02", decision="non_gold", actual_option="C")
        ),
        sensitivity.ModelAssistedDecision(
            **_model_decision_row(
                question_id="q_db_03", decision="keep_implemented_gold", actual_option="B"
            )
        ),
        sensitivity.ModelAssistedDecision(**_model_decision_row(question_id="q_db_04")),
        sensitivity.ModelAssistedDecision(**_model_decision_row(question_id="q_db_05")),
    ]

    fully = sensitivity._fully_adjudicated_corrections(
        observed=observed,
        author_adjudicated=author_adjudicated,
        full_audit=full_audit,
        decisions=model_decisions,
    )

    labels = {row.question_id: row.new_label for row in fully}
    assert labels == {
        "q_db_01": "absent_or_ambiguous",
        "q_db_02": "implemented_non_gold",
        "q_db_04": "absent_or_ambiguous",
        "q_db_05": "absent_or_ambiguous",
    }
    assert {row.key for row in author_adjudicated} <= {row.key for row in fully}
    assert {row.key for row in fully} <= {row.key for row in worst_case}
    by_question = {row.question_id: row for row in fully}
    assert by_question["q_db_02"].provenance == (
        "full_conformance_audit_both_passes|model_assisted_adjudication"
    )
    assert by_question["q_db_02"].confidence == "model_assisted_high"
    assert "agree; 2026-10-16; author_approved on 2026-10-16)" in by_question["q_db_02"].reason
    summary = sensitivity._model_assisted_summary(
        decisions=model_decisions,
        observed=observed,
        full_audit=full_audit,
        author_adjudicated=author_adjudicated,
        fully_adjudicated=fully,
    )
    assert (summary["model_relabels"], summary["kept_implemented_gold"]) == (3, 1)
    assert [
        row["question_id"] for row in summary["labels_differing_from_proposed_sensitivity_label"]
    ] == ["q_db_02"]

    with pytest.raises(RuntimeError, match="kept implemented_gold on observed"):
        sensitivity._fully_adjudicated_corrections(
            observed=observed,
            author_adjudicated=author_adjudicated,
            full_audit=full_audit,
            decisions=[
                sensitivity.ModelAssistedDecision(
                    **_model_decision_row(question_id="q_db_04", decision="keep_implemented_gold")
                )
            ],
        )


def test_model_assisted_decisions_csv_covers_remaining_flags_with_provenance() -> None:
    audit_dir = ROOT / "rebuttal" / "full_conformance_audit"
    path = audit_dir / "model_assisted_decisions.csv"
    raw = path.read_bytes()
    assert b"\r" not in raw
    assert b"/Users/" not in raw and b"/private/" not in raw
    with path.open(newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))
    with (audit_dir / "author_decisions.csv").open(newline="", encoding="utf-8") as handle:
        author_rows = list(csv.DictReader(handle))
    with (audit_dir / "disagreements.csv").open(newline="", encoding="utf-8") as handle:
        flags = list(csv.DictReader(handle))

    author = sensitivity._load_author_decisions(author_rows, disagreement_rows=flags)
    decisions = sensitivity._load_model_assisted_decisions(
        rows, disagreement_rows=flags, author_decisions=author
    )

    assert len(decisions) == 162
    assert {decision.key for decision in decisions} | {decision.key for decision in author} == {
        (row["task_id"], row["artifact_id"], row["question_id"]) for row in flags
    }
    assert {
        (decision.decided_by, decision.decided_on, decision.review_status, decision.reviewed_on)
        for decision in decisions
    } == {
        (
            "model-assisted adjudication (Claude), two passes with arbitration",
            "2026-10-16",
            "author_approved",
            "2026-10-16",
        )
    }
    assert Counter(decision.decision for decision in decisions) == Counter(
        {"absent_or_ambiguous": 151, "keep_implemented_gold": 8, "non_gold": 3}
    )
    assert Counter(decision.pass_agreement for decision in decisions) == Counter(
        {"agree": 155, "arbitrated": 7}
    )


def test_adjudicated_sensitivity_baseline_uses_v1_1_labels() -> None:
    output_dir = ROOT / "rebuttal" / "full_conformance_audit" / "label_sensitivity_adjudicated"
    with (output_dir / "implementation_coverage.csv").open(newline="", encoding="utf-8") as handle:
        rows = {
            (row["scenario"], row["scope_id"], row["grain"], row["entity_id"]): row
            for row in csv.DictReader(handle)
        }

    # Paper Table 11: the MiMo 2.5-low build without source files is labeled absent.
    mimo_low = rows[("baseline", "artifact_present_all", "builder", "claude_code_mimo_v2_5_low")]
    assert (mimo_low["artifact_question_pairs"], mimo_low["implemented_gold"]) == ("60", "42")
    compile_pass = rows[
        ("baseline", "compile_pass_primary", "builder", "claude_code_mimo_v2_5_low")
    ]
    assert (compile_pass["artifact_question_pairs"], compile_pass["implemented_gold"]) == (
        "45",
        "42",
    )


def test_generated_population_manifest_has_requested_row_grain() -> None:
    path = ROOT / "rebuttal" / "full_conformance_audit" / "population_manifest.csv"
    with path.open(newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))

    assert len(rows) == 592
    assert len({(row["task_id"], row["artifact_id"], row["question_id"]) for row in rows}) == 592


def test_integrity_attestation_is_required_and_detects_result_drift(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(audit, "ROOT", tmp_path)
    output_dir = tmp_path / "audit"
    output_dir.mkdir()
    (output_dir / "SPEC.md").write_text("protocol v1.2\n", encoding="utf-8")
    artifact_dir = tmp_path / "artifact"
    artifact_dir.mkdir()
    (artifact_dir / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
    pair = audit.Pair(
        source={
            "task_id": "scratch_minidb",
            "artifact_id": "build-test",
            "artifact_path": "artifact",
            "question_id": "q_db_test",
        },
        question={"correct_option": "B"},
    )
    attempt_dir = output_dir / "raw" / "primary" / "scratch_minidb" / "build-test" / "attempt-001"
    attempt_dir.mkdir(parents=True)
    prompt = "trace the live path\n"
    assignment = "write result.json\n"
    (attempt_dir / "prompt.md").write_text(prompt, encoding="utf-8")
    (attempt_dir / "assignment.md").write_text(assignment, encoding="utf-8")
    payload = {
        "task_id": "scratch_minidb",
        "artifact_id": "build-test",
        "rows": [
            {
                "question_id": "q_db_test",
                "verdict": "confirmed_gold",
                "actual_option": "B",
                "confidence": "high",
                "runtime_reachability_checked": True,
                "evidence_files": ["main.rs"],
                "evidence_symbols": ["main"],
                "evidence_spans": [{"file": "main.rs", "start_line": 1, "end_line": 1}],
                "rationale": "The live entry point is main and uniquely matches B.",
            }
        ],
    }
    audit.write_json(attempt_dir / "result.json", payload)
    request = {
        "execution_engine": "collaboration.spawn_agent",
        "pass_id": "primary",
        "task_id": "scratch_minidb",
        "artifact_id": "build-test",
        "artifact_path": "artifact",
        "question_ids": ["q_db_test"],
        "model_id": audit.PRIMARY_CONFIG["model_id"],
        "reasoning_effort": audit.PRIMARY_CONFIG["reasoning_effort"],
        "prompt_version": audit.PROMPT_VERSION,
        "rendered_prompt_sha256": audit.sha256_file(attempt_dir / "prompt.md"),
        "assignment_prompt_sha256": audit.sha256_file(attempt_dir / "assignment.md"),
        "agent_task_name": "full_audit_p01",
    }
    audit.write_json(attempt_dir / "request.json", request)
    completion = {
        **request,
        "status": "validated",
        "session_id": "full_audit_p01",
        "result_sha256": audit.sha256_file(attempt_dir / "result.json"),
        "selected_result_sha256": audit.sha256_file(attempt_dir / "result.json"),
    }
    audit.write_json(attempt_dir / "completion.json", completion)

    with pytest.raises(RuntimeError, match="lacks integrity attestation"):
        audit.verify_integrity_attestation(
            output_dir=output_dir,
            attempt_dir=attempt_dir,
            pass_id="primary",
            task_id="scratch_minidb",
            artifact_id="build-test",
        )
    attestation = audit.create_integrity_attestation(
        output_dir=output_dir,
        attempt_dir=attempt_dir,
        pass_id="primary",
        task_id="scratch_minidb",
        artifact_id="build-test",
        expected_pairs=[pair],
    )
    assert attestation["result_hash_source"] == ("completion_record_and_parent_verified")
    audit.verify_integrity_attestation(
        output_dir=output_dir,
        attempt_dir=attempt_dir,
        pass_id="primary",
        task_id="scratch_minidb",
        artifact_id="build-test",
    )

    with (attempt_dir / "result.json").open("a", encoding="utf-8") as handle:
        handle.write("\n")
    with pytest.raises(RuntimeError, match="attested result hash drift"):
        audit.verify_integrity_attestation(
            output_dir=output_dir,
            attempt_dir=attempt_dir,
            pass_id="primary",
            task_id="scratch_minidb",
            artifact_id="build-test",
        )


def test_generation_pointer_switches_only_to_complete_immutable_bundle(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(audit, "ROOT", tmp_path)
    output_dir = tmp_path / "audit"
    output_dir.mkdir()
    audit.write_json(output_dir / "execution_manifest.json", {"frozen": True})
    audit.write_json(
        output_dir / "integrity_attestation_manifest.json",
        {"validated_attempt_count": 82},
    )
    (output_dir / "SPEC.md").write_text("protocol v1.2\n", encoding="utf-8")
    core_contents = {
        "minidb_adjudication.csv": "task_id\nscratch_minidb\n",
    }
    core_generation_id = audit.publish_core_generation(
        output_dir=output_dir,
        contents=core_contents,
        validation={"population_pairs": 592},
    )
    contents = {
        **core_contents,
        "report.md": "# report\n",
    }

    generation_id = audit.publish_generation(
        output_dir=output_dir,
        contents=contents,
        validation={"population_pairs": 592},
    )

    pointer = json.loads((output_dir / "CURRENT_GENERATION.json").read_text(encoding="utf-8"))
    generation_dir = output_dir / "generations" / generation_id
    assert pointer["current_generation_id"] == generation_id
    core_pointer = json.loads((output_dir / "CORE_GENERATION.json").read_text(encoding="utf-8"))
    assert core_pointer["current_generation_id"] == core_generation_id
    assert (generation_dir / "generation_manifest.json").is_file()
    assert (generation_dir / "minidb_adjudication.csv").read_text(encoding="utf-8") == contents[
        "minidb_adjudication.csv"
    ]
    assert (
        audit.publish_generation(
            output_dir=output_dir,
            contents=contents,
            validation={"population_pairs": 592},
        )
        == generation_id
    )
    assert (
        audit.publish_core_generation(
            output_dir=output_dir,
            contents=core_contents,
            validation={"population_pairs": 592},
        )
        == core_generation_id
    )


def test_stale_or_pre_versioned_sensitivity_manifest_is_not_embedded(
    tmp_path: Path,
) -> None:
    sensitivity_dir = tmp_path / "label_sensitivity"
    sensitivity_dir.mkdir()
    (sensitivity_dir / "rb_rankings.csv").write_text(
        "scenario,panel_id,builder_id,rank\n",
        encoding="utf-8",
    )
    (sensitivity_dir / "implementation_coverage.csv").write_text(
        "scenario,scope_id,grain,implementation_coverage\n",
        encoding="utf-8",
    )
    audit.write_json(
        sensitivity_dir / "manifest.json",
        {
            "analysis_id": "rebuttal_label_sensitivity",
            "input_paths": {},
            "input_sha256": {},
        },
    )
    assert (
        audit.validated_sensitivity_bundle(
            tmp_path,
            expected_disagreement_sha256="a" * 64,
            expected_author_flag_count=205,
        )
        is None
    )

    audit.write_json(
        sensitivity_dir / "manifest.json",
        {
            "manifest_version": audit.SENSITIVITY_MANIFEST_VERSION,
            "analysis_id": "rebuttal_label_sensitivity",
            "analysis_script": audit.display_path(
                ROOT / "scripts" / "analyze_rebuttal_label_sensitivity.py"
            ),
            "analysis_script_sha256": "0" * 64,
        },
    )
    assert (
        audit.validated_sensitivity_bundle(
            tmp_path,
            expected_disagreement_sha256="a" * 64,
            expected_author_flag_count=205,
        )
        is None
    )
