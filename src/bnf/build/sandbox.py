"""Workspace preparation for Build runs.

Each Build run receives an isolated directory. For ``edit_existing_project``
tasks we copy the seed project into the workspace before the agent starts.
"""

from __future__ import annotations

import shutil
from pathlib import Path

from bnf.schema import BuildTaskSpec


def prepare_workspace(
    spec: BuildTaskSpec,
    workspace: Path,
    *,
    specs_dir: Path,
) -> Path:
    """Create ``workspace`` (clean) and populate it with the seed project
    when applicable. Returns ``workspace``.
    """

    if workspace.exists():
        shutil.rmtree(workspace)
    workspace.mkdir(parents=True, exist_ok=False)

    if spec.task_family == "edit_existing_project" and spec.seed_project is not None:
        seed = (specs_dir / spec.seed_project.path).resolve()
        if not seed.exists():
            raise FileNotFoundError(f"seed project {seed} does not exist")
        _copy_tree(seed, workspace)
    return workspace


def _copy_tree(src: Path, dst: Path) -> None:
    for item in src.rglob("*"):
        rel = item.relative_to(src)
        target = dst / rel
        if item.is_dir():
            target.mkdir(parents=True, exist_ok=True)
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(item, target)
