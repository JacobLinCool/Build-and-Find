#!/usr/bin/env python3
"""Prepare runs/ for anonymous public release.

Copies runs/ to runs_release/, sanitizing absolute workspace paths and
excluding legacy / draft / log files. Original runs/ is never modified.
"""

# ruff: noqa: E402

from __future__ import annotations

import hashlib
import json
import os
import shutil
import csv
from collections.abc import Sequence
from pathlib import Path
import sys

REPO_ROOT = Path(__file__).resolve().parent.parent
SRC = REPO_ROOT / "runs"
DST = REPO_ROOT / "runs_release"
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from bnf.analysis.tables import EMPTY_ARTIFACT_MARKER

STRIP_PREFIXES_ENV = "BNF_RELEASE_STRIP_PREFIXES"
FORBIDDEN_SUBSTRINGS_ENV = "BNF_RELEASE_FORBIDDEN_SUBSTRINGS"

EXCLUDE_PATHS: set[str] = {
    "_orchestrator.log",
    "cargo_target",
    "find_workspaces",
    "logs",
    "reports/summary.md",
    "reports/report.csv",
    "reports/report.json",
    "reports/wrongs.json",
    "reports/finder_panel_selection.md",
    "reports/eandd_final/paper_narrative_zh_draft.md",
}
EXCLUDE_NAMES: set[str] = {
    ".DS_Store",
    ".git",
    "node_modules",
    "target",
}


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def _env_items(name: str) -> tuple[str, ...] | None:
    raw = os.environ.get(name)
    if raw is None:
        return None

    items = tuple(item.strip() for item in raw.split(os.pathsep) if item.strip())
    if not items:
        raise SystemExit(f"{name} is set but empty")
    return items


def strip_prefixes() -> tuple[str, ...]:
    configured = _env_items(STRIP_PREFIXES_ENV)
    prefixes = configured if configured is not None else (REPO_ROOT.as_posix(),)
    normalized = (prefix.rstrip("/\\") + "/" for prefix in prefixes)
    return tuple(sorted(normalized, key=len, reverse=True))


def forbidden_substrings() -> tuple[str, ...]:
    configured = _env_items(FORBIDDEN_SUBSTRINGS_ENV)
    if configured is not None:
        return configured

    home = Path.home()
    if home.name and REPO_ROOT.is_relative_to(home):
        return (home.name,)
    return ()


def strip_prefix(path: Path, prefixes: Sequence[str]) -> int:
    text = path.read_text(encoding="utf-8")
    new = text
    replacements = 0
    for prefix in prefixes:
        replacements += new.count(prefix)
        new = new.replace(prefix, "")
    if new == text:
        return 0
    path.write_text(new, encoding="utf-8")
    return replacements


def release_artifact_path(value: str) -> str:
    if not value:
        return value
    return (DST.name + "/artifacts/" + Path(value).name)


def sanitize_run_record(path: Path) -> bool:
    obj = json.loads(path.read_text(encoding="utf-8"))
    changed = False

    artifact_path = obj.get("artifact_path")
    if isinstance(artifact_path, str) and artifact_path:
        new_artifact_path = release_artifact_path(artifact_path)
        if new_artifact_path != artifact_path:
            obj["artifact_path"] = new_artifact_path
            changed = True

    usage = obj.get("usage")
    if isinstance(usage, dict) and "trace_path" in usage:
        del usage["trace_path"]
        changed = True

    if not changed:
        return False
    path.write_text(json.dumps(obj, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return True


def drop_trace_path(path: Path) -> bool:
    obj = json.loads(path.read_text(encoding="utf-8"))
    usage = obj.get("usage")
    if not isinstance(usage, dict) or "trace_path" not in usage:
        return False
    del usage["trace_path"]
    path.write_text(
        json.dumps(obj, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    return True


def copy_tree() -> None:
    if DST.exists():
        shutil.rmtree(DST)

    src_root = SRC.resolve()

    def ignore(dirpath: str, names: list[str]) -> list[str]:
        rel = Path(dirpath).resolve().relative_to(src_root)
        skipped: list[str] = []
        for name in names:
            entry = (rel / name).as_posix()
            if entry in EXCLUDE_PATHS or name in EXCLUDE_NAMES:
                skipped.append(name)
        return skipped

    shutil.copytree(SRC, DST, ignore=ignore)


def strip_all_text_prefixes(prefixes: Sequence[str]) -> int:
    replacements = 0
    for path in sorted(DST.rglob("*")):
        if not path.is_file():
            continue
        try:
            replacements += strip_prefix(path, prefixes)
        except UnicodeDecodeError:
            continue
    return replacements


def redact_forbidden_substrings(values: Sequence[str]) -> int:
    replacements = 0
    for path in sorted(DST.rglob("*")):
        if not path.is_file():
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        new = text
        for index, value in enumerate(values, start=1):
            replacements += new.count(value)
            new = new.replace(value, f"REDACTED_TOKEN_{index}")
        if new != text:
            path.write_text(new, encoding="utf-8")
    return replacements


def rewrite_release_root_references() -> int:
    replacements = 0
    mapping = {
        "runs/artifacts/": f"{DST.name}/artifacts/",
        "runs/records/": f"{DST.name}/records/",
        "runs/reports/": f"{DST.name}/reports/",
        "runs/tables/": f"{DST.name}/tables/",
    }
    for path in sorted(DST.rglob("*")):
        if not path.is_file():
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        new = text
        for old, replacement in mapping.items():
            replacements += new.count(old)
            new = new.replace(old, replacement)
        if new != text:
            path.write_text(new, encoding="utf-8")
    return replacements


def sanitize_records() -> int:
    records_dir = DST / "records"
    if not records_dir.exists():
        raise SystemExit(f"release source is missing raw records: {records_dir}")
    changed = 0
    for path in sorted(records_dir.glob("*.json")):
        if sanitize_run_record(path):
            changed += 1
    return changed


def final_release_record_ids() -> set[str]:
    from scripts.analyze_eandd_final import (
        CONTROL_CONDITIONS,
        DEFAULT_AGENTS,
        DEFAULT_TASKS,
        _control_build_id,
        _load_control_finds,
        _load_final_build_panel,
        _load_final_finds,
        _split_csv,
    )

    records_dir = DST / "records"
    task_ids = _split_csv(DEFAULT_TASKS)
    agent_ids = _split_csv(DEFAULT_AGENTS)
    builds, build_exclusions = _load_final_build_panel(records_dir, task_ids, agent_ids)
    diagnostic_builds = [exclusion.as_build_cell() for exclusion in build_exclusions]
    finds = _load_final_finds(records_dir, builds, agent_ids)
    diagnostic_finds = _load_final_finds(records_dir, diagnostic_builds, agent_ids)
    controls = _load_control_finds(records_dir, task_ids, agent_ids)

    keep = {build.run.build_run_id for build in builds}
    keep.update(build.run.build_run_id for build in diagnostic_builds)
    keep.update(find.run.find_run_id for find in finds)
    keep.update(find.run.find_run_id for find in diagnostic_finds)
    keep.update(control.run.find_run_id for control in controls)
    keep.update(
        _control_build_id(task_id, condition)
        for task_id in task_ids
        for condition in CONTROL_CONDITIONS
    )
    return keep


def prune_to_final_release_panel() -> tuple[int, int, int, int]:
    keep = final_release_record_ids()
    records_dir = DST / "records"
    artifacts_dir = DST / "artifacts"

    removed_records = 0
    removed_record_logs = 0
    for path in sorted(records_dir.iterdir()):
        stem = path.name.removesuffix(".logs")
        if path.is_file() and path.suffix == ".json":
            stem = path.stem
        if stem in keep:
            continue
        if path.is_dir():
            shutil.rmtree(path)
            removed_record_logs += 1
        elif path.is_file():
            path.unlink()
            removed_records += 1

    kept_build_ids = {
        record_id
        for record_id in keep
        if record_id.startswith("build-") or record_id.startswith("control-build-")
    }
    removed_artifacts = 0
    removed_artifact_logs = 0
    if artifacts_dir.exists():
        for path in sorted(artifacts_dir.iterdir()):
            stem = path.name.removesuffix(".logs")
            if stem in kept_build_ids:
                continue
            if path.is_dir():
                shutil.rmtree(path)
                if path.name.endswith(".logs"):
                    removed_artifact_logs += 1
                else:
                    removed_artifacts += 1
            elif path.is_file():
                path.unlink()
                removed_artifacts += 1

    return removed_records, removed_record_logs, removed_artifacts, removed_artifact_logs


def sanitize_build_table() -> int:
    path = DST / "tables" / "build_runs.csv"
    if not path.exists():
        return 0
    rows: list[dict[str, str]] = []
    changed = 0
    with path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        if reader.fieldnames is None:
            raise SystemExit(f"empty CSV table: {path}")
        fieldnames = reader.fieldnames
        for row in reader:
            old = row.get("artifact_path", "")
            new = release_artifact_path(old)
            if new != old:
                row["artifact_path"] = new
                changed += 1
            rows.append(row)
    if changed:
        with path.open("w", newline="", encoding="utf-8") as handle:
            writer = csv.DictWriter(handle, fieldnames=fieldnames, extrasaction="raise")
            writer.writeheader()
            writer.writerows(rows)
    return changed


def ensure_empty_artifact_markers() -> int:
    records_dir = DST / "records"
    markers = 0
    for record_path in sorted(records_dir.glob("build-*.json")):
        record = json.loads(record_path.read_text(encoding="utf-8"))
        artifact_path = record.get("artifact_path")
        if not isinstance(artifact_path, str) or not artifact_path:
            continue
        artifact = REPO_ROOT / artifact_path
        if not artifact.is_dir():
            continue
        artifact_files = [
            path
            for path in artifact.rglob("*")
            if path.is_file() and path.name != EMPTY_ARTIFACT_MARKER
        ]
        if artifact_files:
            continue
        marker = artifact / EMPTY_ARTIFACT_MARKER
        marker.write_text(
            "Release packaging marker for an empty generated artifact. "
            "Ignored by artifact_tree_hash.\n",
            encoding="utf-8",
        )
        markers += 1
    return markers


def refresh_table_manifest() -> None:
    manifest_path = DST / "tables" / "MANIFEST.json"
    if not manifest_path.exists():
        raise SystemExit(f"release source is missing table manifest: {manifest_path}")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest["table_dir"] = f"{DST.name}/tables"
    for fname, info in manifest["files"].items():
        fpath = DST / "tables" / fname
        info["bytes"] = fpath.stat().st_size
        info["sha256"] = sha256_of(fpath)
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


def main() -> None:
    print(f"Copy {SRC.relative_to(REPO_ROOT)} -> {DST.relative_to(REPO_ROOT)}")
    copy_tree()
    changed_records = sanitize_records()
    print(f"  release records sanitized: {changed_records}")
    pruned = prune_to_final_release_panel()
    print(
        "  non-final evidence pruned: "
        f"{pruned[0]} records, {pruned[1]} record logs, "
        f"{pruned[2]} artifacts, {pruned[3]} artifact logs"
    )
    prefixes = strip_prefixes()
    print(f"  configured prefix sanitizers: {len(prefixes)}")

    n = strip_all_text_prefixes(prefixes)
    print(f"  prefix-strip: {n} replacements")
    release_root_rewrites = rewrite_release_root_references()
    print(f"  release root references rewritten: {release_root_rewrites}")
    sensitive_values = forbidden_substrings()
    redactions = redact_forbidden_substrings(sensitive_values)
    print(f"  forbidden substring redactions: {redactions}")
    table_path_rewrites = sanitize_build_table()
    print(f"  release table artifact paths rewritten: {table_path_rewrites}")
    empty_markers = ensure_empty_artifact_markers()
    print(f"  empty artifact markers ensured: {empty_markers}")

    recovery_dirs = [
        DST / "reports/eandd_final/nested_answer_recovery_originals",
        DST / "reports/eandd_final/malformed_answer_recovery_originals",
    ]
    dropped = 0
    for d in recovery_dirs:
        if not d.exists():
            continue
        for f in sorted(d.glob("*.json")):
            if drop_trace_path(f):
                dropped += 1
    print(f"  trace_path dropped: {dropped} recovery JSONs")

    refresh_table_manifest()
    print("  MANIFEST.json: table_dir + sha256 refreshed")

    print("\n=== Verification ===")
    leaks: list[tuple[Path, int, int]] = []
    for f in DST.rglob("*"):
        if not f.is_file():
            continue
        try:
            data = f.read_text(encoding="utf-8", errors="ignore")
        except Exception:
            continue
        for index, value in enumerate(sensitive_values, start=1):
            n = data.count(value)
            if n:
                leaks.append((f.relative_to(REPO_ROOT), index, n))
    if leaks:
        print("WARN: residual forbidden substring matches:")
        for p, index, n in leaks:
            print(f"  {p}: token#{index}: {n}")
    elif sensitive_values:
        print("Clean: no configured forbidden substrings in release tree.")
    else:
        print(f"Skipped: set {FORBIDDEN_SUBSTRINGS_ENV} to verify private substrings.")

    total_files = sum(1 for p in DST.rglob("*") if p.is_file())
    total_bytes = sum(p.stat().st_size for p in DST.rglob("*") if p.is_file())
    print(f"\n{total_files} files, {total_bytes / 1024 / 1024:.1f} MB in {DST.relative_to(REPO_ROOT)}/")


if __name__ == "__main__":
    main()
