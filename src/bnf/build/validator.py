"""Post-build artefact probe.

The benchmark does not validate correctness here. We only care whether
the produced codebase compiles (so the Find phase sees a coherent
project) and capture a couple of cheap structural signals for the run
log. Find-phase accuracy is the real metric.

All cargo invocations run with ``CARGO_TARGET_DIR`` pointed at the
caller-supplied shared scratch directory so the artefact itself stays
source-only.
"""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path

from bnf.schema import BuildTaskSpec
from bnf.schema.run import ValidationResult


def _run(
    cmd: list[str],
    cwd: Path,
    *,
    env: dict[str, str] | None = None,
    timeout: int = 1200,
) -> tuple[int, str, str]:
    try:
        proc = subprocess.run(
            cmd,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env,
            check=False,
        )
    except FileNotFoundError as e:
        return 127, "", f"missing binary: {e}"
    except subprocess.TimeoutExpired:
        return 124, "", f"timeout running {' '.join(cmd)}"
    return proc.returncode, proc.stdout, proc.stderr


def _count_rust(workspace: Path) -> tuple[int, int]:
    files = 0
    loc = 0
    for rs in workspace.rglob("*.rs"):
        parts = set(rs.parts)
        if "target" in parts or ".git" in parts:
            continue
        files += 1
        try:
            loc += sum(1 for _ in rs.open("r", encoding="utf-8", errors="replace"))
        except OSError:
            continue
    return files, loc


def validate_artifact(
    spec: BuildTaskSpec,
    workspace: Path,
    *,
    specs_dir: Path,
    cargo_target_dir: Path | None = None,
) -> ValidationResult:
    result = ValidationResult()
    details: dict[str, str] = {}

    files, loc = _count_rust(workspace)
    result.rust_files = files
    result.rust_loc = loc
    details["rust_files"] = str(files)
    details["rust_loc"] = str(loc)

    if os.environ.get("BNF_DRY_RUN") == "1":
        result.compile_pass = bool(files)
        details["dry_run"] = "skipped cargo"
        result.details = details
        return result

    if not (workspace / "Cargo.toml").exists():
        result.details = {**details, "error": "no Cargo.toml at workspace root"}
        return result
    if shutil.which("cargo") is None:
        result.details = {**details, "error": "cargo is not installed on PATH"}
        return result

    env = os.environ.copy()
    if cargo_target_dir is not None:
        cargo_target_dir.mkdir(parents=True, exist_ok=True)
        env["CARGO_TARGET_DIR"] = str(cargo_target_dir.resolve())

    rc, _, err = _run(["cargo", "build", "--release"], workspace, env=env)
    result.compile_pass = rc == 0
    details["build"] = f"rc={rc}\n{err.strip()[-800:]}"
    result.details = details
    return result
