from __future__ import annotations

import hashlib
import importlib.util
import json
import sys
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]


def load_script_module(name: str, relative_path: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative_path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


croissant = load_script_module("update_croissant_metadata", "scripts/update_croissant_metadata.py")

URL = "https://huggingface.co/datasets/example/build-and-find"


def _metadata(*distribution: dict) -> dict:
    return {"url": URL, "version": "1.1.0", "distribution": list(distribution)}


def test_content_path_maps_the_release_revision_prefix() -> None:
    metadata = _metadata()
    assert croissant.resolve_prefix(metadata) == f"{URL}/resolve/v1.1.0/"
    assert (
        croissant.content_path(metadata, f"{URL}/resolve/v1.1.0/runs_release/tables/a.csv")
        == "runs_release/tables/a.csv"
    )
    with pytest.raises(croissant.CroissantLayoutError):
        croissant.content_path(metadata, "runs_release/tables/a.csv")
    with pytest.raises(croissant.CroissantLayoutError):
        croissant.content_path(metadata, f"{URL}/resolve/v1.0.0/runs_release/tables/a.csv")


def test_content_path_accepts_relative_and_other_revisions_when_not_strict() -> None:
    metadata = _metadata()
    for value in ("runs_release/tables/a.csv", f"{URL}/resolve/main/runs_release/tables/a.csv"):
        assert croissant.content_path(metadata, value, strict=False) == "runs_release/tables/a.csv"
    for value in (
        "../outside.csv",
        "/abs/a.csv",
        "runs_release//a.csv",
        "https://example.org/a.csv",
        f"{URL}/resolve/v1.1.0",
        f"{URL}/resolve/v1.1.0/a.csv?download=true",
    ):
        with pytest.raises(croissant.CroissantLayoutError):
            croissant.content_path(metadata, value, strict=False)


def test_refresh_file_objects_canonicalizes_urls_and_contains_file_sets(tmp_path: Path) -> None:
    data = b"a,b\n1,2\n"
    (tmp_path / "tables").mkdir()
    (tmp_path / "tables" / "a.csv").write_bytes(data)
    metadata = _metadata(
        {
            "@type": "cr:FileObject",
            "@id": "repo",
            "contentUrl": "stale",
            "encodingFormat": "git+https",
            "sha256": "https://github.com/mlcommons/croissant/issues/80",
        },
        {"@type": "cr:FileObject", "@id": "a", "contentUrl": "tables/a.csv"},
        {"@type": "cr:FileSet", "@id": "tables", "includes": "tables/*.csv"},
    )
    assert croissant.refresh_file_objects(metadata, root=tmp_path)
    repo, item, file_set = metadata["distribution"]
    assert repo["contentUrl"] == URL
    assert item["contentUrl"] == f"{URL}/resolve/v1.1.0/tables/a.csv"
    assert item["contentSize"] == str(len(data))
    assert item["sha256"] == hashlib.sha256(data).hexdigest()
    assert file_set["containedIn"] == {"@id": "repo"}
    assert not croissant.refresh_file_objects(metadata, root=tmp_path)


def test_release_croissant_points_at_the_hosted_dataset_files() -> None:
    metadata = json.loads((ROOT / "metadata" / "croissant.json").read_text(encoding="utf-8"))
    repositories = croissant.repository_ids(metadata)
    assert len(repositories) == 1
    for item in metadata["distribution"]:
        if item["@type"] == "cr:FileSet":
            assert item["containedIn"] == {"@id": repositories[0]}, item["@id"]
            assert not Path(item["includes"]).is_absolute()
        elif croissant.is_repository_object(item):
            assert item["contentUrl"] == croissant.dataset_url(metadata)
        else:
            path = croissant.content_path(metadata, item["contentUrl"])
            assert (ROOT / path).is_file(), path
