from __future__ import annotations

import csv
import importlib.util
import json
import sys
from collections.abc import Callable
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
RELEASE = ROOT / "runs_release"
RELEASE_OUTPUT_DIR = RELEASE / "reports" / "option_free_check"


def load_script_module(name: str, relative_path: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative_path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


ofc = load_script_module("analyze_option_free_check", "scripts/analyze_option_free_check.py")

MINIDB_ARTIFACT = "build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3"
NANOWEB_ARTIFACT = "build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601"


# --------------------------------------------------------------------------- fixtures


def make_record(
    run_id: str,
    condition: str,
    *,
    question_ids: tuple[str, ...] = ("q_db_01", "q_db_05"),
    trial_id: str = "0",
) -> object:
    artifact = condition == ofc.ARTIFACT_OPEN
    answers = {
        question: {
            "question_id": question,
            "question_prompt": f"Prompt for {question}?",
            "claim": f"Claim about {question}.",
            "evidence_files": ["src/main.rs", "src/cli.rs"] if artifact else [],
            "evidence_symbols": ["main"] if artifact else [],
            "rationale": "One short sentence.",
            "answered": True,
        }
        for question in question_ids
    }
    return ofc.Record(
        run_id=run_id,
        condition=condition,
        task_id="scratch_minidb",
        finder_id="codex_gpt5_5_high",
        trial_id=trial_id,
        seed="20260724",
        source_build_run_id=MINIDB_ARTIFACT if artifact else "",
        created_at="2026-07-23T18:00:00+00:00",
        question_ids=question_ids,
        answers=answers,
        vendor_total_tokens=100,
        wall_clock_seconds=1.5,
    )


Rows = list[dict[str, str]]


def write_csv(path: Path, fields: tuple[str, ...], rows: Rows) -> Path:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(fields), lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)
    return path


def write_grading(
    tmp_path: Path,
    records: dict,
    mutate: Callable[[Rows, Rows, Rows, Rows], None] | None = None,
) -> dict[str, Path]:
    cells = sorted(
        ((record, question) for record in records.values() for question in record.question_ids),
        key=lambda item: ofc.shuffle_hash(item[0].run_id, item[1]),
    )
    keys: Rows = []
    pass1: Rows = []
    pass2: Rows = []
    llm: Rows = []
    for index, (record, question) in enumerate(cells, start=1):
        grading_id = f"g_{index:04d}"
        evidence = "sufficient" if record.condition == ofc.ARTIFACT_OPEN else "not_applicable"
        keys.append(
            {
                "grading_id": grading_id,
                "open_find_run_id": record.run_id,
                "condition": record.condition,
                "finder_id": record.finder_id,
                "trial_id": record.trial_id,
                "seed": record.seed,
                "source_build_run_id": record.source_build_run_id,
            }
        )
        row = {"shuffle_index": str(index), "grading_id": grading_id}
        row.update(ofc.sheet_content(record, question))
        pass1.append(
            {**row, "claim_correct": "correct", "evidence_sufficient": "", "grader_note": ""}
        )
        pass2.append(
            {**row, "claim_correct": "correct", "evidence_sufficient": evidence, "grader_note": ""}
        )
        llm.append(
            {
                "grading_id": grading_id,
                "claim_correct": "incorrect",
                "evidence_sufficient": evidence,
                "note": "",
            }
        )
    if mutate is not None:
        mutate(keys, pass1, pass2, llm)
    return {
        "grading_key": write_csv(tmp_path / "grading_key.csv", ofc.KEY_FIELDS, keys),
        "grading_pass1": write_csv(tmp_path / "pass1.csv", ofc.SHEET_FIELDS, pass1),
        "grading_pass2": write_csv(tmp_path / "pass2.csv", ofc.SHEET_FIELDS, pass2),
        "llm_labels": write_csv(tmp_path / "llm.csv", ofc.LLM_FIELDS, llm),
    }


def two_records() -> dict:
    records = [
        make_record("openfind-a", ofc.ARTIFACT_OPEN),
        make_record("openfind-q", ofc.QUESTION_ONLY_OPEN),
    ]
    return {record.run_id: record for record in records}


def response(
    condition: str, question: str, correct: bool, *, task: str = "scratch_minidb"
) -> object:
    return ofc.Response(
        grading_id=f"{condition}-{question}",
        run_id="r",
        condition=condition,
        finder_id="codex_gpt5_5_high",
        trial_id="0",
        task_id=task,
        question_id=question,
        claim_label="correct" if correct else "incorrect",
        pass2_claim_label="correct" if correct else "incorrect",
        evidence_label="sufficient",
        cites_evidence=condition == ofc.ARTIFACT_OPEN,
        llm_claim_label="correct",
        llm_evidence_label="sufficient",
        has_note=False,
    )


# --------------------------------------------------------------------------- unit tests


def test_load_responses_joins_key_and_frozen_sheets(tmp_path: Path) -> None:
    records = two_records()
    responses = ofc.load_responses(write_grading(tmp_path, records), records)
    assert len(responses) == 4
    by_condition = {row.condition: row for row in responses}
    assert by_condition[ofc.ARTIFACT_OPEN].cites_evidence
    assert not by_condition[ofc.QUESTION_ONLY_OPEN].cites_evidence
    assert all(row.correct for row in responses)
    grading = ofc.grading_summary(responses)
    assert grading.correctness_unchanged.count == "4/4"
    assert grading.artifact_sufficient.count == "2/2"
    assert grading.llm_correctness_agreement.count == "0/4"
    assert grading.llm_correct_author_incorrect == 0
    assert grading.llm_incorrect_author_correct == 4


def _edit_claim(keys: Rows, pass1: Rows, pass2: Rows, llm: Rows) -> None:
    pass1[0]["claim"] = "Edited claim."


def _swap_order(keys: Rows, pass1: Rows, pass2: Rows, llm: Rows) -> None:
    for sheet in (pass1, pass2):
        sheet[0]["shuffle_index"], sheet[1]["shuffle_index"] = (
            sheet[1]["shuffle_index"],
            sheet[0]["shuffle_index"],
        )


def _pass1_evidence(keys: Rows, pass1: Rows, pass2: Rows, llm: Rows) -> None:
    pass1[0]["evidence_sufficient"] = "sufficient"


def _wrong_condition(keys: Rows, pass1: Rows, pass2: Rows, llm: Rows) -> None:
    keys[0]["condition"] = (
        ofc.QUESTION_ONLY_OPEN if keys[0]["condition"] == ofc.ARTIFACT_OPEN else ofc.ARTIFACT_OPEN
    )


def _unknown_label(keys: Rows, pass1: Rows, pass2: Rows, llm: Rows) -> None:
    pass1[0]["claim_correct"] = "partial"


@pytest.mark.parametrize(
    ("mutate", "message"),
    [
        (_edit_claim, "differs from the record"),
        (_swap_order, "shuffle rule"),
        (_pass1_evidence, "evidence label"),
        (_wrong_condition, "grading key does not match"),
        (_unknown_label, "is not one of"),
    ],
)
def test_load_responses_rejects_inconsistent_sheets(tmp_path: Path, mutate, message) -> None:
    records = two_records()
    with pytest.raises(ValueError, match=message):
        ofc.load_responses(write_grading(tmp_path, records, mutate), records)


def test_pass2_correctness_changes_are_counted(tmp_path: Path) -> None:
    records = two_records()

    def flip(keys: Rows, pass1: Rows, pass2: Rows, llm: Rows) -> None:
        pass2[0]["claim_correct"] = "incorrect"

    responses = ofc.load_responses(write_grading(tmp_path, records, flip), records)
    assert ofc.grading_summary(responses).correctness_unchanged.count == "3/4"


def test_load_records_requires_records_without_trace_path(tmp_path: Path) -> None:
    run_id = "openfind-scratch_minidb-question_only_open-codex_gpt5_5_high-t0-s1-abc"
    record = {
        "record_type": "open_find_run",
        "open_find_run_id": run_id,
        "condition": ofc.QUESTION_ONLY_OPEN,
        "task_id": "scratch_minidb",
        "source_build_run_id": None,
        "question_ids": ["q_db_01"],
        "manifest": {
            "agent": {"id": "codex_gpt5_5_high"},
            "trial_id": 0,
            "seed": 1,
            "created_at": "2026-07-23T18:00:00+00:00",
        },
        "answers": [{"question_id": "q_db_01", "question_prompt": "P?", "claim": "C."}],
        "usage": {"wall_clock_seconds": 2.0, "vendor_usage": {"total_tokens": 7}},
        "status": "ok",
        "error": None,
    }
    path = tmp_path / f"{run_id}.json"
    path.write_text(json.dumps(record), encoding="utf-8")
    loaded = ofc.load_records([path])[run_id]
    assert (loaded.trial_id, loaded.seed, loaded.vendor_total_tokens) == ("0", "1", 7)

    record["usage"]["trace_path"] = "/somewhere/session.jsonl"
    path.write_text(json.dumps(record), encoding="utf-8")
    with pytest.raises(ValueError, match="trace_path"):
        ofc.load_records([path])

    record["usage"].pop("trace_path")
    record["source_build_run_id"] = MINIDB_ARTIFACT
    path.write_text(json.dumps(record), encoding="utf-8")
    with pytest.raises(ValueError, match="source artifact"):
        ofc.load_records([path])


def test_artifact_rule_replays_the_frozen_selection() -> None:
    def build(build_id: str, task: str, family: str, compile_pass: str = "true") -> dict:
        return {
            "build_run_id": build_id,
            "task_id": task,
            "builder_family": family,
            "compile_pass": compile_pass,
        }

    builds = [
        build("b-minidb-mimo-2", "scratch_minidb", "mimo"),
        build("b-minidb-mimo-1", "scratch_minidb", "mimo"),
        build("b-minidb-mimo-0", "scratch_minidb", "mimo", compile_pass="false"),
        build("b-minidb-codex", "scratch_minidb", "codex"),
        build("b-minidb-mimo-partial", "scratch_minidb", "mimo"),
        build("b-nanoweb-mimo", "scratch_nanoweb", "mimo"),
    ]
    labels = [
        {"artifact_id": build_id, "final_label": "implemented_gold"}
        for build_id in (
            "b-minidb-mimo-2",
            "b-minidb-mimo-1",
            "b-minidb-mimo-0",
            "b-minidb-codex",
            "b-nanoweb-mimo",
        )
        for _ in range(3)
    ]
    labels += [
        {"artifact_id": "b-minidb-mimo-partial", "final_label": label}
        for label in ("implemented_gold", "implemented_gold", "absent_or_ambiguous")
    ]
    used = {"scratch_minidb": "b-minidb-mimo-1", "scratch_nanoweb": "b-nanoweb-mimo"}
    rule = ofc.artifact_rule(builds, labels, finder_families={"codex", "claude"}, used=used)
    assert rule.eligible == {
        "scratch_minidb": ["b-minidb-mimo-1", "b-minidb-mimo-2"],
        "scratch_nanoweb": ["b-nanoweb-mimo"],
    }
    assert rule.reproduced
    other = ofc.artifact_rule(
        builds, labels, finder_families={"codex", "claude"}, used={**used, "scratch_minidb": "x"}
    )
    assert not other.reproduced


def test_question_selection_drops_only_negative_contrast_items() -> None:
    frozen = [("scratch_minidb", "q_db_01"), ("scratch_minidb", "q_db_07")]
    types = {
        ("scratch_minidb", "q_db_01"): "capability_recognition",
        ("scratch_minidb", "q_db_07"): "negative_contrast",
    }
    excluded = ofc.check_question_selection(frozen, types, {"scratch_minidb": ("q_db_01",)})
    assert excluded == (("scratch_minidb", "q_db_07"),)
    with pytest.raises(ValueError, match="negative-contrast"):
        ofc.check_question_selection(frozen, types, {"scratch_minidb": ("q_db_01", "q_db_07")})


def test_sensitivity_drops_relabeled_pairs_from_both_conditions() -> None:
    responses = [
        response(ofc.ARTIFACT_OPEN, "q_db_01", True),
        response(ofc.ARTIFACT_OPEN, "q_db_05", False),
        response(ofc.QUESTION_ONLY_OPEN, "q_db_01", False),
        response(ofc.QUESTION_ONLY_OPEN, "q_db_05", True),
    ]
    labels = [
        ofc.PairLabel(
            task_id="scratch_minidb",
            question_id=question,
            artifact_id=MINIDB_ARTIFACT,
            released="implemented_gold",
            flag="",
            decision="",
            scenario_labels={
                ofc.OBSERVED_ERRORS: observed,
                ofc.ADJUDICATED: adjudicated,
            },
        )
        for question, observed, adjudicated in (
            ("q_db_01", "implemented_gold", "implemented_gold"),
            ("q_db_05", "implemented_gold", "implemented_non_gold"),
        )
    ]
    rows = {row.scenario: row for row in ofc.sensitivities(responses, labels)}
    assert rows[ofc.OBSERVED_ERRORS].changed_pairs == ()
    assert rows[ofc.OBSERVED_ERRORS].comparison.artifact_open.count == "1/2"
    assert rows[ofc.ADJUDICATED].changed_pairs == (("scratch_minidb", "q_db_05"),)
    assert rows[ofc.ADJUDICATED].comparison.artifact_open.count == "1/1"
    assert rows[ofc.ADJUDICATED].comparison.question_only_open.count == "0/1"
    assert ofc.signed_points(rows[ofc.ADJUDICATED].comparison.difference) == "+100.0"


# --------------------------------------------------------------------------- release replay


@pytest.fixture(scope="module")
def analysis():
    if (
        not (RELEASE / "tables" / "question_answers.csv").exists()
        or not (RELEASE_OUTPUT_DIR / "grading_key.csv").exists()
    ):
        pytest.skip("runs_release option-free check is not available")
    return ofc.analyze(ofc.Paths(root=ROOT, runs_root=RELEASE), input_dir=RELEASE_OUTPUT_DIR)


@pytest.fixture(scope="module")
def printed(analysis) -> dict[str, str]:
    return {value.value_id: value.printed for value in analysis.printed}


def test_release_design_and_artifact_rule(analysis, printed) -> None:
    assert len(analysis.records) == 24
    assert len(analysis.responses) == 96
    assert analysis.rule.used == {
        "scratch_minidb": MINIDB_ARTIFACT,
        "scratch_nanoweb": NANOWEB_ARTIFACT,
    }
    assert analysis.rule.reproduced
    assert printed["design.questions"] == "8 of 10"
    assert printed["design.excluded_questions"] == "q_db_07, q_nw_11"


def test_release_headline_counts(analysis, printed) -> None:
    assert printed["option_free.artifact_open"] == "38/48"
    assert printed["option_free.question_only_open"] == "13/48"
    assert printed["option_free.difference_pp"] == "+52.1"
    assert printed["mcq.artifact_present"] == "48/48"
    assert printed["mcq.question_only"] == "48/48"
    assert printed["mcq.question_only_runs_excluded"] == "0"
    assert [
        (
            printed[f"finder.{finder}.artifact_open"],
            printed[f"finder.{finder}.question_only_open"],
        )
        for finder in ofc.FINDERS
    ] == [("19/24", "5/24"), ("19/24", "8/24")]
    assert [
        (printed[f"task.{task}.artifact_open"], printed[f"task.{task}.question_only_open"])
        for task in ofc.TASK_IDS
    ] == [("18/24", "5/24"), ("20/24", "8/24")]
    assert (
        printed["question.q_nw_07.artifact_open"],
        printed["question.q_nw_07.question_only_open"],
    ) == (
        "2/6",
        "3/6",
    )


def test_release_low_prior_and_label_sensitivities(analysis, printed) -> None:
    assert printed["low_prior.still_low_prior"] == "4 of 8"
    assert printed["low_prior.questions"] == "q_db_06, q_nw_04, q_nw_07, q_nw_10"
    assert (printed["low_prior.artifact_open"], printed["low_prior.question_only_open"]) == (
        "18/24",
        "3/24",
    )
    assert printed["sensitivity.observed_errors.questions"] == "q_db_05, q_nw_07"
    assert (
        printed["sensitivity.observed_errors.artifact_open"],
        printed["sensitivity.observed_errors.question_only_open"],
        printed["sensitivity.observed_errors.difference_pp"],
    ) == ("32/36", "9/36", "+63.9")
    assert printed["sensitivity.adjudicated.questions"] == "q_db_01, q_db_05, q_nw_07"
    assert (
        printed["sensitivity.adjudicated.artifact_open"],
        printed["sensitivity.adjudicated.question_only_open"],
        printed["sensitivity.adjudicated.difference_pp"],
    ) == ("28/30", "9/30", "+63.3")
    assert analysis.overlay_corrections == {"observed_errors": 9, "adjudicated": 195}
    assert [
        (row.task_id, row.released_gold, row.flagged, row.adjudicated_gold)
        for row in analysis.artifacts
    ] == [("scratch_minidb", 15, 6, 9), ("scratch_nanoweb", 15, 10, 5)]


def test_release_grading_counts(printed) -> None:
    assert printed["grading.correctness_unchanged"] == "96/96"
    assert printed["grading.artifact_open_evidence_sufficient"] == "48/48"
    assert printed["grading.pass1_evidence_rows.artifact_open"] == "48/48"
    assert printed["grading.pass1_evidence_rows.question_only_open"] == "0/48"
    assert printed["grading.question_only_no_evidence_cited"] == "48"
    assert printed["grading.llm_correctness_agreement"] == "82/96"
    assert printed["grading.llm_disagreements"] == "14"
    assert printed["grading.llm_disagreements_author_stricter"] == "12"
    assert printed["grading.llm_evidence_agreement"] == "96/96"
    assert printed["compute.vendor_tokens"] == "9.85M"


def test_release_dates(analysis, printed) -> None:
    assert printed["dates.option_free_sessions"] == "2026-07-23"
    assert printed["dates.mcq_runs"] == "2026-05-02"
    assert printed["dates.option_free_sessions_month"] == "July 2026"
    assert printed["dates.mcq_runs_month"] == "May 2026"
    summary = ofc._summary(analysis)
    assert summary["compute"]["first_session_created_at"] == "2026-07-23T18:25:46.219907+00:00"
    assert "generated_at" not in summary
    assert summary["table_manifest_generated_at"] == analysis.generated_at


def test_month_text_requires_one_calendar_month() -> None:
    assert ofc._month_text(["2026-07-23T18:25:46+00:00", "2026-07-31"]) == "July 2026"
    assert ofc._month_text(["2026-12-01"]) == "December 2026"
    with pytest.raises(ValueError, match="one calendar month"):
        ofc._month_text(["2026-07-31", "2026-08-01"])


def test_committed_outputs_are_current(analysis, tmp_path: Path) -> None:
    ofc.write_outputs(analysis, tmp_path)
    for name in ("summary.json", "report.md"):
        generated = (tmp_path / name).read_text(encoding="utf-8")
        committed = (RELEASE_OUTPUT_DIR / name).read_text(encoding="utf-8")
        assert str(ROOT) not in generated
        if name == "summary.json":
            # Input hashes depend on the checkout's line endings; compare everything else.
            generated_json, committed_json = json.loads(generated), json.loads(committed)
            generated_json.pop("input_hashes")
            committed_json.pop("input_hashes")
            assert generated_json == committed_json
        else:
            assert generated == committed


def test_release_copy_has_no_host_paths_or_session_logs() -> None:
    if not RELEASE_OUTPUT_DIR.exists():
        pytest.skip("runs_release option-free check is not available")
    allowed = {
        "README.md",
        "grading_key.csv",
        "grading_sheet.pass1.csv",
        "grading_sheet.pass2.csv",
        "llm_prelabels.csv",
        "report.md",
        "rubric.md",
        "summary.json",
    }
    files = [path for path in RELEASE_OUTPUT_DIR.rglob("*") if path.is_file()]
    top_level = {path.name for path in files if path.parent == RELEASE_OUTPUT_DIR}
    records = [path for path in files if path.parent == RELEASE_OUTPUT_DIR / "records"]
    assert top_level == allowed
    assert len(records) == 24 and all(path.suffix == ".json" for path in records)
    assert len(files) == len(allowed) + len(records)
    for path in files:
        data = path.read_bytes()
        text = data.decode("utf-8")
        assert b"\r\n" not in data, path.name
        for needle in (str(ROOT), "/Users/", "/home/", "/var/folders", "/private/"):
            assert needle not in text, (path.name, needle)
    for path in records:
        assert "trace_path" not in json.loads(path.read_text(encoding="utf-8"))["usage"]
