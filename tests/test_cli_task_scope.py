"""CLI task-scope defaults.

The headline benchmark scope is from-scratch synthesis. Edit tasks remain
available as explicit opt-ins, but must not leak into default run/analyze
paths.
"""

from __future__ import annotations

from types import SimpleNamespace

import pytest
import typer

from bnf.cli import (
    PRIMARY_TASK_FAMILY,
    _load_scoped_runs,
    _resolve_task_ids,
    _resolve_trial_counts,
)
from bnf.schema.run import (
    AgentInfo,
    BuildRun,
    FinderAnswer,
    FindRun,
    RunManifest,
    UsageMetrics,
    ValidationResult,
    write_run,
)


def _spec(task_id: str, task_family: str):
    return SimpleNamespace(task_id=task_id, task_family=task_family)


def test_default_task_scope_is_scratch_only() -> None:
    specs = [
        _spec("scratch_minidb", PRIMARY_TASK_FAMILY),
        _spec("edit_minidb_agg_join", "edit_existing_project"),
    ]

    assert _resolve_task_ids(specs, tasks=None, include_edit=False) == {"scratch_minidb"}


def test_task_scope_can_include_or_explicitly_select_edit_tasks() -> None:
    specs = [
        _spec("scratch_minidb", PRIMARY_TASK_FAMILY),
        _spec("edit_minidb_agg_join", "edit_existing_project"),
    ]

    assert _resolve_task_ids(specs, tasks=None, include_edit=True) == {
        "scratch_minidb",
        "edit_minidb_agg_join",
    }
    assert _resolve_task_ids(specs, tasks="edit_minidb_agg_join", include_edit=False) == {
        "edit_minidb_agg_join"
    }


def test_task_scope_rejects_unknown_explicit_tasks() -> None:
    specs = [_spec("scratch_minidb", PRIMARY_TASK_FAMILY)]

    with pytest.raises(typer.BadParameter):
        _resolve_task_ids(specs, tasks="scratch_minidb,missing", include_edit=False)

    with pytest.raises(typer.BadParameter):
        _resolve_task_ids(specs, tasks=",", include_edit=False)


def test_run_all_supports_split_build_and_find_trial_counts() -> None:
    assert _resolve_trial_counts(trials=None, build_trials=2, find_trials=3) == (2, 3)
    assert _resolve_trial_counts(trials=4, build_trials=None, find_trials=None) == (4, 4)

    with pytest.raises(typer.BadParameter):
        _resolve_trial_counts(trials=2, build_trials=2, find_trials=None)


def _manifest(run_id: str, agent_id: str) -> RunManifest:
    return RunManifest(
        run_id=run_id,
        spec_hash="h",
        prompt_hash="p",
        agent=AgentInfo(id=agent_id, adapter="codex", model_id="m"),
        seed=0,
        trial_id=0,
    )


def _build_run(run_id: str, task_id: str) -> BuildRun:
    return BuildRun(
        build_run_id=run_id,
        task_id=task_id,
        manifest=_manifest(run_id, "builder"),
        artifact_path="/tmp/unused",
        usage=UsageMetrics(),
        validation=ValidationResult(compile_pass=True),
        status="ok",
    )


def _find_run(run_id: str, build_run_id: str, task_id: str) -> FindRun:
    return FindRun(
        find_run_id=run_id,
        build_run_id=build_run_id,
        task_id=task_id,
        manifest=_manifest(run_id, "finder"),
        answers=[
            FinderAnswer(
                question_id="q1",
                presented_options={"A": "a", "B": "b", "C": "c", "D": "d"},
                presented_correct_key="A",
                selected_option="A",
                is_correct=True,
            )
        ],
        usage=UsageMetrics(),
        status="ok",
    )


def test_load_scoped_runs_filters_existing_edit_records(tmp_path, monkeypatch) -> None:
    monkeypatch.setenv("BNF_ROOT", str(tmp_path))
    runs_dir = tmp_path / "runs" / "records"
    scratch_task = "scratch_minidb"
    edit_task = "edit_minidb_agg_join"

    write_run(_build_run("build-scratch", scratch_task), runs_dir)
    write_run(_build_run("build-edit", edit_task), runs_dir)
    write_run(_find_run("find-scratch", "build-scratch", scratch_task), runs_dir)
    write_run(_find_run("find-edit", "build-edit", edit_task), runs_dir)

    build_runs, find_runs = _load_scoped_runs(task_ids={scratch_task})

    assert [br.task_id for br in build_runs] == [scratch_task]
    assert [fr.task_id for fr in find_runs] == [scratch_task]
