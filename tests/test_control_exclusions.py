from __future__ import annotations

import csv
import importlib.util
import json
import random
import sys
from fractions import Fraction
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
RELEASE = ROOT / "runs_release"
RELEASE_OUTPUT_DIR = RELEASE / "reports" / "eandd_final" / "control_exclusions"


def load_script_module(name: str, relative_path: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative_path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


ce = load_script_module("analyze_control_exclusions", "scripts/analyze_control_exclusions.py")
released_bootstrap = load_script_module(
    "analyze_eandd_calibration_uncertainty",
    "scripts/analyze_eandd_calibration_uncertainty.py",
)


def answer(
    *,
    finder: str = "f1",
    task: str = "scratch_minidb",
    trial: str = "0",
    question: str = "q1",
    correct: bool = True,
    run: str | None = None,
    build: str = "b1",
    panels: tuple[str, ...] = ("question_only",),
):
    return ce.Answer(
        find_run_id=run or f"{finder}-{task}-t{trial}",
        build_run_id=build,
        task_id=task,
        question_id=question,
        finder_id=finder,
        finder_trial_id=trial,
        correct=correct,
        panels=frozenset(panels),
    )


def write_exclusions(path: Path, rows: list[dict[str, str]]) -> Path:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(ce.EXCLUSION_FIELDS))
        writer.writeheader()
        writer.writerows(rows)
    return path


def exclusion_row(**overrides: str) -> dict[str, str]:
    row = {
        "find_run_id": "f1-scratch_minidb-t0",
        "finder_id": "f1",
        "task_id": "scratch_minidb",
        "finder_trial_id": "0",
        "exposure": "seed_project",
        "evidence": "Read seed project files.",
    }
    row.update(overrides)
    return row


def test_rounding_is_half_up_on_exact_rationals() -> None:
    assert ce.percent(Fraction(2844, 2880)) == "98.8"
    assert ce.is_half_tie(100 * Fraction(2844, 2880))
    assert ce.percent(Fraction(98749, 100000)) == "98.7"
    # 0.0225 * 100 is 2.2499999999999996 in binary floating point.
    assert ce.signed_points(Fraction(9, 400)) == "+2.3"
    assert ce.signed_points(Fraction(-9, 400)) == "-2.3"
    assert ce.signed_points(Fraction(-1, 300)) == "-0.3"
    assert not ce.is_half_tie(100 * Fraction(-1, 300))


def test_load_exclusions_validates_columns_exposure_and_duplicates(tmp_path: Path) -> None:
    good = write_exclusions(tmp_path / "good.csv", [exclusion_row()])
    runs = ce.load_exclusions(good)
    assert [run.stratum for run in runs] == [("f1", "scratch_minidb", "0")]

    bad_exposure = write_exclusions(tmp_path / "bad.csv", [exclusion_row(exposure="web")])
    with pytest.raises(ValueError, match="unknown exposure"):
        ce.load_exclusions(bad_exposure)

    duplicate = write_exclusions(tmp_path / "dup.csv", [exclusion_row(), exclusion_row()])
    with pytest.raises(ValueError, match="listed twice"):
        ce.load_exclusions(duplicate)

    no_evidence = write_exclusions(tmp_path / "empty.csv", [exclusion_row(evidence=" ")])
    with pytest.raises(ValueError, match="missing evidence"):
        ce.load_exclusions(no_evidence)

    wrong_columns = tmp_path / "columns.csv"
    wrong_columns.write_text("find_run_id,exposure\nx,seed_project\n", encoding="utf-8")
    with pytest.raises(ValueError, match="expected columns"):
        ce.load_exclusions(wrong_columns)


def test_validate_exclusions_requires_a_matching_question_only_run(tmp_path: Path) -> None:
    control = [answer(trial="0"), answer(trial="1")]
    runs = ce.load_exclusions(write_exclusions(tmp_path / "x.csv", [exclusion_row()]))
    ce.validate_exclusions(runs, control)

    unknown = ce.load_exclusions(
        write_exclusions(tmp_path / "u.csv", [exclusion_row(find_run_id="missing")])
    )
    with pytest.raises(ValueError, match="not a question-only control run"):
        ce.validate_exclusions(unknown, control)

    mismatched = ce.load_exclusions(
        write_exclusions(tmp_path / "m.csv", [exclusion_row(finder_trial_id="2")])
    )
    with pytest.raises(ValueError, match="does not match"):
        ce.validate_exclusions(mismatched, control)


def test_matched_strata_drop_only_the_same_finder_task_and_trial(tmp_path: Path) -> None:
    runs = ce.load_exclusions(write_exclusions(tmp_path / "x.csv", [exclusion_row()]))
    strata = ce.excluded_strata(runs)
    treatment = [
        answer(trial="0", panels=("paper_final_scratch",), run="a"),
        answer(trial="1", panels=("paper_final_scratch",), run="b"),
        answer(task="scratch_nanoweb", trial="0", panels=("paper_final_scratch",), run="c"),
        answer(finder="f2", trial="0", panels=("paper_final_scratch",), run="d"),
    ]
    kept = ce.drop_strata(treatment, strata)
    assert [row.find_run_id for row in kept] == ["b", "c", "d"]

    control = [answer(trial="0"), answer(trial="1")]
    remaining = ce.drop_runs(control, {run.find_run_id for run in runs})
    assert [row.finder_trial_id for row in remaining] == ["1"]


def test_bootstrap_contrast_replays_the_released_bootstrap() -> None:
    rng = random.Random(7)
    treatment, control = [], []
    for index in range(7):
        question = f"q{index}"
        for _ in range(rng.randrange(20, 40)):
            treatment.append(answer(question=question, correct=rng.random() < 0.9))
        for _ in range(rng.randrange(3, 9)):
            control.append(answer(question=question, correct=rng.random() < 0.8))
    control.append(answer(question="control_only", correct=False))

    def as_rows(rows):
        return [
            {"task_id": row.task_id, "question_id": row.question_id, "is_correct": str(row.correct)}
            for row in rows
        ]

    expected = released_bootstrap._bootstrap_row(
        "unit_contrast", treatment_rows=as_rows(treatment), control_rows=as_rows(control)
    )
    contrast = ce.bootstrap_contrast("unit_contrast", treatment, control)
    assert contrast.clusters == expected["clusters"] == 7
    assert contrast.treatment_rows == expected["treatment_answer_rows"]
    assert contrast.control_rows == expected["control_answer_rows"]
    for field, value in (
        ("difference", contrast.difference),
        ("ci_low", contrast.ci_low),
        ("ci_high", contrast.ci_high),
    ):
        assert f"{float(value):.6f}" == expected[field]


def test_bootstrap_streams_are_named_per_task() -> None:
    assert ce.contrast_stream("compile_pass_minus_question_only") == (
        "compile_pass_minus_question_only"
    )
    assert ce.contrast_stream("compile_pass_minus_question_only", "scratch_minidb") == (
        "compile_pass_minus_question_only:scratch_minidb"
    )


def test_question_agreement_keeps_remaining_attempts() -> None:
    rows = [
        answer(finder="f1", trial="0", correct=True),
        answer(finder="f2", trial="0", correct=True),
        answer(finder="f2", trial="1", correct=False),
        answer(finder="f2", trial="2", correct=True),
        answer(finder="f3", trial="0", correct=True),
        answer(finder="f3", trial="1", correct=True),
        answer(finder="f3", trial="2", correct=True),
    ]
    stats = ce.question_agreement(rows, lambda row: row.finder_id)
    assert stats == {("scratch_minidb", "q1"): (2, 3, 1, 3)}


def test_load_labels_applies_v1_1_corrections(tmp_path: Path) -> None:
    final = tmp_path / "final.csv"
    final.write_text(
        "task_id,artifact_id,question_id,final_label,label_source\n"
        "t,a,q1,implemented_gold,consensus_prelabel\n"
        "t,a,q2,implemented_gold,manual_review\n",
        encoding="utf-8",
    )
    corrections = tmp_path / "corrections.csv"
    corrections.write_text(
        "task_id,artifact_id,question_id,old_label,new_label,label_source,rationale\n"
        "t,a,q1,implemented_gold,absent_or_ambiguous,rule_empty_artifact,empty\n",
        encoding="utf-8",
    )
    labels, sources = ce.load_labels(final, corrections)
    assert labels == {("t", "a", "q1"): "absent_or_ambiguous", ("t", "a", "q2"): "implemented_gold"}
    assert sources == {"rule_empty_artifact": 1, "manual_review": 1}

    corrections.write_text(
        "task_id,artifact_id,question_id,old_label,new_label,label_source,rationale\n"
        "t,a,q1,implemented_non_gold,absent_or_ambiguous,rule_empty_artifact,empty\n",
        encoding="utf-8",
    )
    with pytest.raises(RuntimeError, match="does not match"):
        ce.load_labels(final, corrections)

    corrections.write_text(
        "task_id,artifact_id,question_id,old_label,new_label,label_source,rationale\n"
        "t,a,q1,implemented_gold,absent_or_ambiguous,rule_empty_artifact,empty\n"
        "t,a,q1,implemented_gold,absent_or_ambiguous,rule_empty_artifact,empty\n",
        encoding="utf-8",
    )
    with pytest.raises(RuntimeError, match="duplicate"):
        ce.load_labels(final, corrections)

    corrections.write_text(
        "task_id,artifact_id,question_id,old_label,new_label,label_source,rationale\n"
        "t,a,q1,implemented_gold,not_a_label,rule_empty_artifact,empty\n",
        encoding="utf-8",
    )
    with pytest.raises(RuntimeError, match="unknown corrected label"):
        ce.load_labels(final, corrections)


# --------------------------------------------------------------------------- release data

TABLE1 = {
    # slice: (builds, find runs, all-row accuracy, delta, impl.-gold accuracy, perfect)
    "question_only": (None, 61, "97.2", None, None, None),
    "spec_only": (None, 72, "99.9", "+2.7 [+1.0, +5.0]", None, None),
    "artifact_present": (48, 1728, "96.4", "-0.7 [-3.7, +2.3]", "98.9", "86.5"),
    "compile_pass": (41, 1476, "96.7", "-0.4 [-3.2, +2.6]", "98.9", "86.4"),
    "compile_failed": (7, 252, "94.4", "-2.7 [-7.2, +1.3]", "98.8", "86.6"),
}
TABLE12 = {
    ("question_only", "scratch_minidb"): (None, 30, "98.2", None, None, None),
    ("question_only", "scratch_nanoweb"): (None, 31, "96.1", None, None, None),
    ("spec_only", "scratch_minidb"): (None, 36, "99.8", "+1.6 [+0.2, +3.1]", None, None),
    ("spec_only", "scratch_nanoweb"): (None, 36, "100.0", "+3.9 [+0.9, +8.0]", None, None),
    ("artifact_present", "scratch_minidb"): (24, 864, "95.8", "-2.3 [-5.0, +0.4]", "98.7", "83.2"),
    ("artifact_present", "scratch_nanoweb"): (24, 864, "96.9", "+0.8 [-4.4, +5.8]", "99.1", "89.9"),
    ("compile_pass", "scratch_minidb"): (21, 756, "96.4", "-1.7 [-4.2, +0.8]", "98.7", "82.7"),
    ("compile_pass", "scratch_nanoweb"): (20, 720, "97.0", "+0.9 [-4.1, +6.0]", "99.2", "90.4"),
}
TABLE8 = [
    # finder: (formal, q-only delta, low-prior delta, impl.-signal delta), in table order
    ("claude_code_sonnet_4_6_low", ("98.7", "+0.9", "+3.4", "+2.2")),
    ("claude_code_sonnet_4_6_high", ("98.0", "+2.5", "+10.2", "+5.6")),
    ("claude_code_opus_4_7_low", ("97.9", "-2.1", "-2.6", "-0.7")),
    ("claude_code_opus_4_7_high", ("97.1", "-2.9", "-3.1", "-0.8")),
    ("claude_code_mimo_v2_5_high", ("96.8", "+1.7", "+9.4", "+5.4")),
    ("claude_code_mimo_v2_5_low", ("96.8", "+2.8", "+12.7", "+7.1")),
    ("claude_code_mimo_v2_5_pro_low", ("96.7", "+12.9", "+36.7", "+23.0")),
    ("claude_code_mimo_v2_5_pro_high", ("96.7", "+5.0", "+19.9", "+10.5")),
    ("codex_gpt5_5_low", ("96.5", "-3.5", "-3.4", "-1.0")),
    ("codex_gpt5_5_high", ("95.9", "-4.1", "-4.2", "-1.5")),
    ("codex_gpt5_4_mini_low", ("95.2", "-2.1", "+0.4", "+0.5")),
    ("codex_gpt5_4_mini_high", ("94.1", "-5.9", "-7.3", "-3.6")),
]
OUTPUT_FILES = (
    "printed_values.csv",
    "contrasts.csv",
    "calibration_rows.csv",
    "repeatability.csv",
    "low_prior_questions.csv",
    "finder_calibration.csv",
    "finder_selection.csv",
    "summary.json",
    "report.md",
)


@pytest.fixture(scope="module")
def analysis():
    if not (RELEASE / "tables" / "question_answers.csv").exists():
        pytest.skip("runs_release tables are not available")
    return ce.analyze(
        ce.Paths(root=ROOT, runs_root=RELEASE),
        exclusions_path=RELEASE_OUTPUT_DIR / ce.EXCLUSIONS_NAME,
    )


@pytest.fixture(scope="module")
def printed(analysis) -> dict[str, str]:
    return {value.value_id: value.printed for value in analysis.printed}


def test_released_bootstrap_lift_is_reproduced(analysis) -> None:
    path = RELEASE / "reports" / "eandd_final" / "validity_stress" / "bootstrap_lift.csv"
    with path.open(encoding="utf-8", newline="") as handle:
        released = list(csv.DictReader(handle))
    replayed = [
        {key: value for key, value in row.items() if key != "reproduced"}
        for row in analysis.released_check
    ]
    assert replayed == released


def test_exclusion_list_covers_eleven_question_only_runs(analysis) -> None:
    exposures = [run.exposure for run in analysis.exclusions]
    assert len(exposures) == 11
    assert {exposure: exposures.count(exposure) for exposure in ce.EXPOSURES} == {
        "answer_bearing": 3,
        "seed_project": 6,
        "other_artifact": 2,
    }
    assert analysis.question_only_runs == {
        "released": 72,
        "corrected": 61,
        "answer_bearing_only": 69,
    }


def test_headline_printed_values(analysis, printed) -> None:
    assert printed["setup.question_only_released"] == "94.5"
    assert printed["setup.question_only_corrected"] == "97.2"
    assert printed["app_c.question_only_answer_bearing_only"] == "96.6"
    assert printed["prior_stress.compile_pass_delta"] == "-0.4 [-3.2, +2.6]"
    assert printed["prior_stress.spec_only_delta"] == "+2.7 [+1.0, +5.0]"
    assert printed["low_prior.question_only"] == "91.8"
    assert printed["low_prior.recovery_all_rows"] == "95.3"
    assert printed["low_prior.delta"] == "+3.8 [-2.6, +10.6]"
    assert printed["low_prior.impl_gold_recovery"] == "97.9"
    assert printed["low_prior.still_low_prior"] == "6 of 10"
    assert (
        printed["table2.question_only.find_runs"],
        printed["table2.question_only.answer_rows"],
        printed["table2.question_only.agreement"],
        printed["table2.question_only.reliable"],
    ) == ("61", "915", "94.4", "24 / 30")
    assert printed["table2.formal.reliable"] == "29 / 30"
    assert printed["table2.spec_only.reliable"] == "30 / 30"
    assert printed["table13.formal.scratch_minidb.reliable"] == "14 / 15"
    assert printed["table13.formal.scratch_nanoweb.reliable"] == "15 / 15"
    assert printed["table1.caption.unscored_runs"] == "36"
    assert printed["app_e.largest_question_only_lift.claude_code_mimo_v2_5_pro_low"] == "+12.9"
    assert analysis.selection.unchanged
    assert analysis.selection.core_finders == [
        "claude_code_sonnet_4_6_low",
        "claude_code_sonnet_4_6_high",
    ]
    assert analysis.selection.audit_finders == ["claude_code_mimo_v2_5_pro_low"]
    assert analysis.label_sources == {
        "consensus_prelabel": 681,
        "manual_review": 24,
        "rule_empty_artifact": 15,
    }


def _formatted(row) -> tuple:
    return (
        row.builds,
        row.find_runs,
        ce.percent(row.all_rows.value),
        None if row.delta is None else row.delta.printed,
        None if row.implemented_gold is None else ce.percent(row.implemented_gold.value),
        None if row.perfect is None else ce.percent(row.perfect.value),
    )


def test_table1_and_table12_rows(analysis) -> None:
    table1 = {
        row.slice_id: _formatted(row) for row in analysis.calibration if row.table == "table1"
    }
    table12 = {
        (row.slice_id, row.task_id): _formatted(row)
        for row in analysis.calibration
        if row.table == "table12"
    }
    assert table1 == TABLE1
    assert table12 == TABLE12


def test_table8_rows_and_order(analysis) -> None:
    rows = [
        (
            finder.finder_id,
            (
                ce.percent(finder.formal_all_rows.value),
                ce.signed_points(finder.delta("all")),
                ce.signed_points(finder.delta("low_prior")),
                ce.signed_points(finder.delta("implementation_signal")),
            ),
        )
        for finder in analysis.finders
    ]
    assert rows == TABLE8


def test_committed_outputs_are_current(analysis, tmp_path: Path) -> None:
    ce.write_outputs(analysis, tmp_path)
    for name in OUTPUT_FILES:
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
            assert generated == committed, name
