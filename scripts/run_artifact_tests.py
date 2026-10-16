"""Run each released build artifact's own test suite and record the results.

The harness compile probe runs only ``cargo build --release``; this script
adds a post-hoc ``cargo test`` sweep over the 48 released scratch build
artifacts. Release build runs first because several integration harnesses
resolve the binary under ``target/release/`` via cargo metadata; running
``cargo test`` alone would spuriously fail them. Both commands pass ``--locked``
so that a rerun fails instead of rewriting a released ``Cargo.lock``; the
recorded run used the same commands without the flag and left every released
lock file unchanged.

Outputs one CSV row per artifact keyed by build_run_id, suitable for
joining against runs_release/tables/build_runs.csv (compile_pass) and
artifact_conformance_final.csv (implementation coverage).
``scripts/summarize_runtime_tests.py`` summarizes the CSV.

Usage:
    uv run python scripts/run_artifact_tests.py \
        [--artifacts-root runs_release/artifacts] \
        [--out runs_release/reports/runtime_tests/artifact_tests.csv] \
        [--target-root /tmp/bnf-artifact-tests-target] \
        [--only <substring>]
"""

from __future__ import annotations

import argparse
import csv
import os
import re
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUT = "runs_release/reports/runtime_tests/artifact_tests.csv"
BUILD_TIMEOUT = 1200
TEST_TIMEOUT = 1200

FIELDNAMES = (
    "build_run_id", "task_id", "cargo_present", "build_rc", "build_ok",
    "build_seconds", "test_rc", "test_ok", "test_seconds", "test_binaries",
    "tests_passed", "tests_failed", "tests_ignored", "note",
)

TEST_RESULT_RE = re.compile(
    r"^test result: (?P<status>\w+)\. (?P<passed>\d+) passed; (?P<failed>\d+) failed; "
    r"(?P<ignored>\d+) ignored",
    re.MULTILINE,
)


def _run(cmd: list[str], cwd: Path, env: dict[str, str], timeout: int) -> tuple[int, str, float]:
    start = time.monotonic()
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
        return proc.returncode, proc.stdout + "\n" + proc.stderr, time.monotonic() - start
    except subprocess.TimeoutExpired:
        return 124, f"timeout after {timeout}s running {' '.join(cmd)}", time.monotonic() - start


def _parse_task(build_run_id: str) -> str:
    m = re.match(r"build-(scratch_minidb|scratch_nanoweb)-", build_run_id)
    return m.group(1) if m else "unknown"


def run_one(artifact: Path, target_root: Path) -> dict[str, object]:
    build_run_id = artifact.name
    task_id = _parse_task(build_run_id)
    row: dict[str, object] = {
        "build_run_id": build_run_id,
        "task_id": task_id,
        "cargo_present": (artifact / "Cargo.toml").exists(),
        "build_rc": "",
        "build_ok": False,
        "build_seconds": "",
        "test_rc": "",
        "test_ok": False,
        "test_seconds": "",
        "test_binaries": 0,
        "tests_passed": 0,
        "tests_failed": 0,
        "tests_ignored": 0,
        "note": "",
    }
    if not row["cargo_present"]:
        # Matches the release-mode compile probe: no root Cargo.toml means the
        # artifact is excluded from the primary panel (nested-crate layouts and
        # one empty artifact; see paper Table 10).
        row["note"] = "no root Cargo.toml (excluded by release-mode compile probe)"
        return row

    env = os.environ.copy()
    target_dir = target_root / task_id
    target_dir.mkdir(parents=True, exist_ok=True)
    env["CARGO_TARGET_DIR"] = str(target_dir.resolve())

    build_rc, build_out, build_s = _run(
        ["cargo", "build", "--release", "--locked"], artifact, env, BUILD_TIMEOUT
    )
    row["build_rc"] = build_rc
    row["build_ok"] = build_rc == 0
    row["build_seconds"] = round(build_s, 1)
    if build_rc != 0:
        row["note"] = "release build failed: " + build_out.strip().splitlines()[-1][:200] if build_out.strip() else "release build failed"
        return row

    test_rc, test_out, test_s = _run(["cargo", "test", "--locked"], artifact, env, TEST_TIMEOUT)
    row["test_rc"] = test_rc
    row["test_ok"] = test_rc == 0
    row["test_seconds"] = round(test_s, 1)

    results = list(TEST_RESULT_RE.finditer(test_out))
    row["test_binaries"] = len(results)
    row["tests_passed"] = sum(int(m.group("passed")) for m in results)
    row["tests_failed"] = sum(int(m.group("failed")) for m in results)
    row["tests_ignored"] = sum(int(m.group("ignored")) for m in results)
    if test_rc != 0:
        failing = [
            line.strip()
            for line in test_out.splitlines()
            if line.strip().startswith("test ") and line.strip().endswith("FAILED")
        ]
        row["note"] = "; ".join(failing[:5])[:400] or ("test run failed rc=" + str(test_rc))
    return row


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts-root", default="runs_release/artifacts")
    parser.add_argument("--out", default=DEFAULT_OUT)
    parser.add_argument("--target-root", default=None, help="shared CARGO_TARGET_DIR root")
    parser.add_argument("--only", default=None, help="only artifacts whose id contains this substring")
    args = parser.parse_args()

    artifacts_root = (REPO_ROOT / args.artifacts_root).resolve()
    out_path = (REPO_ROOT / args.out).resolve()
    out_path.parent.mkdir(parents=True, exist_ok=True)
    target_root = (
        Path(args.target_root).resolve()
        if args.target_root
        else Path("/tmp/bnf-artifact-tests-target")
    )

    artifacts = sorted(
        p
        for p in artifacts_root.iterdir()
        if p.is_dir() and p.name.startswith("build-") and not p.name.endswith(".logs")
    )
    if args.only:
        artifacts = [p for p in artifacts if args.only in p.name]
    if not artifacts:
        print("no artifacts matched", file=sys.stderr)
        return 1

    fieldnames = list(FIELDNAMES)
    rows: list[dict[str, object]] = []
    for i, artifact in enumerate(artifacts, 1):
        print(f"[{i}/{len(artifacts)}] {artifact.name}", flush=True)
        row = run_one(artifact, target_root)
        rows.append(row)
        # Rewrite the CSV after every artifact so partial progress survives interruption.
        with out_path.open("w", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=fieldnames, lineterminator="\n")
            writer.writeheader()
            writer.writerows(rows)
        print(
            f"    build_ok={row['build_ok']} test_ok={row['test_ok']} "
            f"passed={row['tests_passed']} failed={row['tests_failed']} note={row['note']}",
            flush=True,
        )

    ok = sum(1 for r in rows if r["test_ok"])
    print(f"done: {ok}/{len(rows)} artifacts pass their own test suites -> {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
