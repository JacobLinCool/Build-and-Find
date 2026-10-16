#!/usr/bin/env python3
"""Verify that the configured release tree is internally reproducible."""

from __future__ import annotations

import csv
import hashlib
import json
from pathlib import Path
import re
import sys
from typing import Any

from bnf.analysis.tables import artifact_tree_hash
from bnf.config import paths


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.update_croissant_metadata import (  # noqa: E402
    CroissantLayoutError,
    content_path,
    dataset_url,
    is_repository_object,
)

HF_DATASET_URL_PREFIX = "https://huggingface.co/datasets/"
# The dataset URL can still be a template placeholder, filled in before publishing.
URL_PLACEHOLDER_RE = re.compile(r"\{\{[A-Z][A-Z0-9_]*\}\}")
CSV_TABLES = {
    "build_runs.csv",
    "find_runs.csv",
    "question_answers.csv",
    "panel_membership.csv",
}
REQUIRED_LICENSE_FILES = {
    "LICENSE",
    "LICENSE-CODE",
    "LICENSE-DATA",
}
REQUIRED_CROISSANT_FIELDS = {
    "@context",
    "@type",
    "name",
    "description",
    "conformsTo",
    "license",
    "url",
    "version",
    "datePublished",
    "creator",
    "distribution",
    "recordSet",
}
REQUIRED_RAI_FIELDS = {
    "rai:dataLimitations",
    "rai:dataBiases",
    "rai:personalSensitiveInformation",
    "rai:dataUseCases",
    "rai:dataSocialImpact",
    "rai:hasSyntheticData",
    "rai:dataCollection",
    "rai:dataPreprocessingProtocol",
    "rai:dataAnnotationProtocol",
    "rai:dataReleaseMaintenancePlan",
}
REQUIRED_REPORT_PATHS = {
    "runs_release/reports/reproducibility_index.md",
    "runs_release/reports/evidence_citation_audit.md",
    "runs_release/reports/eandd_final/metrics.json",
    "runs_release/reports/eandd_final/insights.md",
    "runs_release/reports/eandd_final/within_finder_efficiency/manifest.json",
    "runs_release/reports/eandd_final/validity_stress/stress_summary.csv",
    "runs_release/reports/eandd_final/validity_stress/bootstrap_lift.csv",
    "runs_release/reports/eandd_final/low_prior/manifest.json",
    "runs_release/reports/eandd_final/control_exclusions/excluded_question_only_runs.csv",
    "runs_release/reports/eandd_final/control_exclusions/summary.json",
    "runs_release/reports/eandd_final/transcript_audit/primary_panel_exposure.csv",
    "runs_release/reports/eandd_final/transcript_audit/summary.json",
    "runs_release/reports/runtime_tests/artifact_tests.csv",
    "runs_release/reports/runtime_tests/summary.json",
    "runs_release/reports/option_free_check/summary.json",
    "runs_release/reports/option_free_check/report.md",
    "runs_release/reports/option_free_check/rubric.md",
    "runs_release/reports/option_free_check/grading_key.csv",
    "runs_release/reports/option_free_check/grading_sheet.pass1.csv",
    "runs_release/reports/option_free_check/grading_sheet.pass2.csv",
    "runs_release/reports/artifact_conformance_consensus/final/label_corrections_v1_1.csv",
    "runs_release/reports/artifact_conformance_consensus/impact/conditioned_builder_scores_by_task.csv",
}


class ReleaseError(RuntimeError):
    pass


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.resolve().as_posix()


def resolve_recorded_path(value: str) -> Path:
    path = Path(value)
    if path.is_absolute():
        return path
    return ROOT / path


def require_release_relative_path(value: str, *, expected_prefix: str, context: str) -> None:
    if Path(value).is_absolute():
        raise ReleaseError(f"{context} must be repository-relative, got absolute path: {value}")
    if not value.startswith(expected_prefix):
        raise ReleaseError(f"{context} must start with {expected_prefix!r}, got {value!r}")


def csv_row_count(path: Path) -> int:
    with path.open(newline="", encoding="utf-8") as handle:
        return sum(1 for _ in csv.DictReader(handle))


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def verify_manifest(table_dir: Path) -> None:
    manifest_path = table_dir / "MANIFEST.json"
    if not manifest_path.exists():
        raise ReleaseError(f"missing table manifest: {display_path(manifest_path)}")

    manifest = load_json(manifest_path)
    expected_table_dir = display_path(table_dir)
    if manifest.get("table_dir") != expected_table_dir:
        raise ReleaseError(
            f"MANIFEST table_dir is {manifest.get('table_dir')!r}; expected {expected_table_dir!r}"
        )

    files = manifest.get("files")
    if not isinstance(files, dict) or not files:
        raise ReleaseError("MANIFEST files must be a non-empty object")

    for name, info in sorted(files.items()):
        path = table_dir / name
        if not path.exists():
            raise ReleaseError(f"manifested table file is missing: {display_path(path)}")
        actual_bytes = path.stat().st_size
        actual_sha = sha256_of(path)
        if info.get("bytes") != actual_bytes:
            raise ReleaseError(f"{name} byte mismatch: manifest={info.get('bytes')} actual={actual_bytes}")
        if info.get("sha256") != actual_sha:
            raise ReleaseError(f"{name} sha256 mismatch: manifest={info.get('sha256')} actual={actual_sha}")
        if name in CSV_TABLES and info.get("row_count") != csv_row_count(path):
            raise ReleaseError(
                f"{name} row_count mismatch: manifest={info.get('row_count')} actual={csv_row_count(path)}"
            )


def verify_license_files() -> None:
    missing = sorted(name for name in REQUIRED_LICENSE_FILES if not (ROOT / name).exists())
    if missing:
        raise ReleaseError(f"missing required license files: {missing}")


def verify_report_paths() -> None:
    missing = sorted(path for path in REQUIRED_REPORT_PATHS if not (ROOT / path).exists())
    if missing:
        raise ReleaseError(f"missing required report paths: {missing}")


def _glob_release_files(pattern: str) -> list[Path]:
    if Path(pattern).is_absolute():
        raise ReleaseError(f"Croissant FileSet includes must be repository-relative: {pattern}")
    return sorted(path for path in ROOT.glob(pattern) if path.is_file())


def verify_croissant_metadata(table_dir: Path) -> None:
    metadata_path = ROOT / "metadata" / "croissant.json"
    if not metadata_path.exists():
        raise ReleaseError(f"missing Croissant metadata: {display_path(metadata_path)}")

    metadata = load_json(metadata_path)
    missing_fields = sorted(REQUIRED_CROISSANT_FIELDS - metadata.keys())
    if missing_fields:
        raise ReleaseError(f"Croissant metadata missing required fields: {missing_fields}")

    missing_rai = sorted(REQUIRED_RAI_FIELDS - metadata.keys())
    if missing_rai:
        raise ReleaseError(f"Croissant metadata missing RAI fields: {missing_rai}")

    license_value = metadata.get("license")
    if license_value not in {
        "https://creativecommons.org/licenses/by/4.0/",
        "https://creativecommons.org/licenses/by/4.0",
    }:
        raise ReleaseError(f"unexpected Croissant dataset license: {license_value!r}")

    distribution = metadata.get("distribution")
    if not isinstance(distribution, list) or not distribution:
        raise ReleaseError("Croissant distribution must be a non-empty list")

    table_manifest = load_json(table_dir / "MANIFEST.json")
    manifest_files = table_manifest.get("files")
    if not isinstance(manifest_files, dict):
        raise ReleaseError("table MANIFEST files must be an object")

    try:
        url = dataset_url(metadata)
    except CroissantLayoutError as error:
        raise ReleaseError(str(error)) from None
    if not URL_PLACEHOLDER_RE.fullmatch(url) and not url.startswith(HF_DATASET_URL_PREFIX):
        raise ReleaseError(f"Croissant url must be a Hugging Face dataset URL, got {url!r}")
    repositories = {
        str(item.get("@id"))
        for item in distribution
        if isinstance(item, dict) and is_repository_object(item)
    }
    if len(repositories) != 1:
        raise ReleaseError(
            f"Croissant distribution must have one git repository FileObject, got {sorted(repositories)}"
        )

    for item in distribution:
        if not isinstance(item, dict):
            raise ReleaseError("Croissant distribution entries must be objects")
        item_type = item.get("@type")
        item_id = item.get("@id", "<unknown>")
        if item_type == "cr:FileObject" and is_repository_object(item):
            if item.get("contentUrl") != url:
                raise ReleaseError(
                    f"Croissant repository {item_id} contentUrl must be the dataset url {url!r}"
                )
        elif item_type == "cr:FileObject":
            content_url = item.get("contentUrl")
            if not isinstance(content_url, str) or not content_url:
                raise ReleaseError(f"Croissant FileObject {item_id} has no contentUrl")
            try:
                relative = content_path(metadata, content_url)
            except CroissantLayoutError as error:
                raise ReleaseError(f"Croissant FileObject {item_id}: {error}") from None
            path = ROOT / relative
            if not path.is_file():
                raise ReleaseError(f"Croissant FileObject missing: {relative}")
            actual_size = path.stat().st_size
            actual_sha = sha256_of(path)
            if str(actual_size) != str(item.get("contentSize")):
                raise ReleaseError(
                    f"Croissant FileObject size mismatch for {relative}: "
                    f"metadata={item.get('contentSize')} actual={actual_size}"
                )
            if item.get("sha256") != actual_sha:
                raise ReleaseError(
                    f"Croissant FileObject sha256 mismatch for {relative}: "
                    f"metadata={item.get('sha256')} actual={actual_sha}"
                )
            if relative.startswith(f"{display_path(table_dir)}/"):
                table_name = Path(relative).name
                if table_name == "MANIFEST.json":
                    continue
                info = manifest_files.get(table_name)
                if not isinstance(info, dict):
                    raise ReleaseError(f"Croissant table {table_name} missing from table manifest")
                if int(info.get("bytes")) != actual_size:
                    raise ReleaseError(f"Croissant/table-manifest byte mismatch for {table_name}")
                if info.get("sha256") != actual_sha:
                    raise ReleaseError(f"Croissant/table-manifest sha256 mismatch for {table_name}")
        elif item_type == "cr:FileSet":
            contained_in = item.get("containedIn")
            if not isinstance(contained_in, dict) or contained_in.get("@id") not in repositories:
                raise ReleaseError(
                    f"Croissant FileSet {item_id} must be containedIn the git repository FileObject"
                )
            includes = item.get("includes")
            if not isinstance(includes, str) or not includes:
                raise ReleaseError(f"Croissant FileSet {item_id} has no includes pattern")
            if not _glob_release_files(includes):
                raise ReleaseError(f"Croissant FileSet {item_id} matched no files: {includes}")

    record_sets = metadata.get("recordSet")
    if not isinstance(record_sets, list) or not record_sets:
        raise ReleaseError("Croissant recordSet must be a non-empty list")
    for record_set in record_sets:
        if not isinstance(record_set, dict):
            raise ReleaseError("Croissant recordSet entries must be objects")
        if not record_set.get("@id") or not record_set.get("field"):
            raise ReleaseError(f"invalid Croissant recordSet entry: {record_set.get('@id')!r}")


def verify_raw_records(records_dir: Path, *, artifact_prefix: str) -> None:
    if not records_dir.exists():
        raise ReleaseError(f"missing raw records directory: {display_path(records_dir)}")
    records = sorted(records_dir.glob("*.json"))
    if not records:
        raise ReleaseError(f"no raw records found under {display_path(records_dir)}")

    build_records = 0
    for path in records:
        record = load_json(path)
        usage = record.get("usage")
        if isinstance(usage, dict) and "trace_path" in usage:
            raise ReleaseError(f"release record still contains usage.trace_path: {display_path(path)}")

        artifact_path = record.get("artifact_path")
        if artifact_path is None:
            continue
        build_records += 1
        if not isinstance(artifact_path, str) or not artifact_path:
            raise ReleaseError(f"invalid artifact_path in {display_path(path)}")
        require_release_relative_path(
            artifact_path,
            expected_prefix=artifact_prefix,
            context=f"{display_path(path)} artifact_path",
        )
        if record.get("status") == "ok" and not resolve_recorded_path(artifact_path).exists():
            raise ReleaseError(
                f"ok build record points to missing artifact: {display_path(path)} -> {artifact_path}"
            )

    if build_records == 0:
        raise ReleaseError(f"no build records with artifact_path found under {display_path(records_dir)}")


def expected_final_record_ids(records_dir: Path) -> set[str]:
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

    task_ids = _split_csv(DEFAULT_TASKS)
    agent_ids = _split_csv(DEFAULT_AGENTS)
    builds, build_exclusions = _load_final_build_panel(records_dir, task_ids, agent_ids)
    diagnostic_builds = [exclusion.as_build_cell() for exclusion in build_exclusions]
    finds = _load_final_finds(records_dir, builds, agent_ids)
    diagnostic_finds = _load_final_finds(records_dir, diagnostic_builds, agent_ids)
    controls = _load_control_finds(records_dir, task_ids, agent_ids)

    expected = {build.run.build_run_id for build in builds}
    expected.update(build.run.build_run_id for build in diagnostic_builds)
    expected.update(find.run.find_run_id for find in finds)
    expected.update(find.run.find_run_id for find in diagnostic_finds)
    expected.update(control.run.find_run_id for control in controls)
    expected.update(
        _control_build_id(task_id, condition)
        for task_id in task_ids
        for condition in CONTROL_CONDITIONS
    )
    return expected


def verify_final_record_set(records_dir: Path) -> None:
    expected = expected_final_record_ids(records_dir)
    actual = {path.stem for path in records_dir.glob("*.json")}
    missing = sorted(expected - actual)
    extra = sorted(actual - expected)
    if missing or extra:
        parts: list[str] = []
        if missing:
            parts.append(f"missing final records: {missing[:8]} ({len(missing)} total)")
        if extra:
            parts.append(f"non-final records present: {extra[:8]} ({len(extra)} total)")
        raise ReleaseError("; ".join(parts))


def verify_artifact_table(table_dir: Path, *, artifact_prefix: str) -> None:
    path = table_dir / "build_runs.csv"
    if not path.exists():
        raise ReleaseError(f"missing build table: {display_path(path)}")

    checked = 0
    with path.open(newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            artifact_path = row.get("artifact_path", "")
            require_release_relative_path(
                artifact_path,
                expected_prefix=artifact_prefix,
                context=f"{display_path(path)} artifact_path for {row.get('build_run_id')}",
            )
            artifact_exists = row.get("artifact_exists") == "true"
            resolved = resolve_recorded_path(artifact_path)
            if artifact_exists and not resolved.exists():
                raise ReleaseError(f"artifact_exists=true but path is absent: {artifact_path}")
            artifact_hash = row.get("artifact_hash", "")
            if artifact_exists and artifact_hash:
                actual_hash = artifact_tree_hash(resolved)
                if actual_hash != artifact_hash:
                    raise ReleaseError(
                        f"artifact hash mismatch for {artifact_path}: table={artifact_hash} actual={actual_hash}"
                    )
            checked += 1

    if checked == 0:
        raise ReleaseError(f"build table has no rows: {display_path(path)}")


def main() -> None:
    p = paths()
    if not p.runs_root.exists():
        raise ReleaseError(f"configured runs root does not exist: {display_path(p.runs_root)}")

    artifact_prefix = f"{display_path(p.runs_root)}/artifacts/"
    verify_license_files()
    verify_raw_records(p.runs_dir, artifact_prefix=artifact_prefix)
    verify_final_record_set(p.runs_dir)
    verify_manifest(p.tables_dir)
    verify_artifact_table(p.tables_dir, artifact_prefix=artifact_prefix)
    verify_report_paths()
    verify_croissant_metadata(p.tables_dir)
    print(f"release verified: {display_path(p.runs_root)}")


if __name__ == "__main__":
    main()
