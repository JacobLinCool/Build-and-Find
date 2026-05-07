"""Filesystem layout and default paths for the bnf CLI."""

from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path

RUNS_ROOT_ENV = "BNF_RUNS_ROOT"


@dataclass(frozen=True)
class Paths:
    root: Path
    runs_root: Path

    @property
    def specs_dir(self) -> Path:
        return self.root / "benchmarks" / "specs"

    @property
    def questions_dir(self) -> Path:
        return self.root / "benchmarks" / "questions"

    @property
    def seeds_dir(self) -> Path:
        return self.root / "benchmarks" / "seed_projects"

    @property
    def artifacts_dir(self) -> Path:
        return self.runs_root / "artifacts"

    @property
    def find_workspaces_dir(self) -> Path:
        return self.runs_root / "find_workspaces"

    @property
    def runs_dir(self) -> Path:
        return self.runs_root / "records"

    @property
    def cargo_target_dir(self) -> Path:
        """Shared CARGO_TARGET_DIR so build artefacts stay source-only.

        Every agent and validator cargo invocation points here via the
        ``CARGO_TARGET_DIR`` env var; this dir can be safely nuked at any
        time to reclaim disk.
        """
        return self.runs_root / "cargo_target"

    @property
    def reports_dir(self) -> Path:
        return self.runs_root / "reports"

    @property
    def tables_dir(self) -> Path:
        return self.runs_root / "tables"


def default_root() -> Path:
    return Path(os.environ.get("BNF_ROOT") or Path.cwd())


def default_runs_root(root: Path) -> Path:
    configured = Path(os.environ.get(RUNS_ROOT_ENV) or "runs")
    if configured.is_absolute():
        return configured.resolve()
    return (root / configured).resolve()


def paths(root: Path | None = None) -> Paths:
    resolved_root = (root or default_root()).resolve()
    return Paths(root=resolved_root, runs_root=default_runs_root(resolved_root))
