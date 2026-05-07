from __future__ import annotations

from pathlib import Path

from bnf.config import paths


def test_paths_default_to_runs_under_root(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.delenv("BNF_RUNS_ROOT", raising=False)

    p = paths(tmp_path)

    assert p.runs_root == tmp_path / "runs"
    assert p.runs_dir == tmp_path / "runs" / "records"
    assert p.tables_dir == tmp_path / "runs" / "tables"


def test_paths_accept_relative_runs_root(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("BNF_RUNS_ROOT", "runs_release")

    p = paths(tmp_path)

    assert p.runs_root == tmp_path / "runs_release"
    assert p.artifacts_dir == tmp_path / "runs_release" / "artifacts"
    assert p.reports_dir == tmp_path / "runs_release" / "reports"
