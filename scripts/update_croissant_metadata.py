#!/usr/bin/env python3
"""Refresh generated fields in metadata/croissant.json.

The Croissant file is published unchanged as ``croissant.json`` at the root of
the Hugging Face dataset, so its file entries point at the hosted files:

- a FileObject ``contentUrl`` is ``<url>/resolve/v<version>/<path>``, where
  ``url`` is the dataset URL of the metadata and ``path`` is the file's
  repository-relative path;
- the git repository of the dataset is a FileObject with ``encodingFormat``
  ``git+https`` and ``contentUrl`` equal to ``url``, and every FileSet is
  ``containedIn`` it, with ``includes`` relative to the repository root.

This script maps each ``contentUrl`` back to the local file to refresh its size
and sha256. A ``contentUrl`` given as a repository-relative path, or under the
``resolve/`` prefix of another revision, is rewritten to the current revision,
and a FileSet without ``containedIn`` is placed in the repository object.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path, PurePosixPath
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
METADATA_PATH = ROOT / "metadata" / "croissant.json"
CROISSANT_SPEC = "http://mlcommons.org/croissant/1.1"
GIT_ENCODING_FORMAT = "git+https"

CONTEXT_1_1_ADDITIONS = {
    "annotation": "cr:annotation",
    "excludes": "cr:excludes",
    "readLines": "cr:readLines",
    "sdVersion": "cr:sdVersion",
    "unArchive": "cr:unArchive",
    "value": "cr:value",
}


class CroissantLayoutError(ValueError):
    """A Croissant file entry does not follow the release layout."""


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_metadata(path: Path) -> dict[str, Any]:
    obj = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(obj, dict):
        raise SystemExit(f"Croissant metadata must be a JSON object: {path}")
    return obj


def dataset_url(metadata: dict[str, Any]) -> str:
    url = metadata.get("url")
    if not isinstance(url, str) or not url.strip("/"):
        raise CroissantLayoutError("Croissant metadata has no dataset url")
    return url.rstrip("/")


def release_revision(metadata: dict[str, Any]) -> str:
    version = metadata.get("version")
    if not isinstance(version, str) or not version:
        raise CroissantLayoutError("Croissant metadata has no version")
    return f"v{version}"


def resolve_prefix(metadata: dict[str, Any]) -> str:
    """URL prefix of the dataset files at the release revision."""
    return f"{dataset_url(metadata)}/resolve/{release_revision(metadata)}/"


def content_url_for(metadata: dict[str, Any], path: str) -> str:
    return resolve_prefix(metadata) + path


def _checked_path(path: str, content_url: str) -> str:
    pure = PurePosixPath(path)
    if (
        not path
        or pure.is_absolute()
        or "\\" in path
        or any(part in {"", ".", ".."} for part in path.split("/"))
        or "://" in path
        or "?" in path
        or "#" in path
    ):
        raise CroissantLayoutError(f"contentUrl does not name a repository file: {content_url}")
    return path


def content_path(metadata: dict[str, Any], content_url: str, *, strict: bool = True) -> str:
    """Repository-relative path named by a FileObject ``contentUrl``.

    With ``strict`` the URL must use the resolve prefix of the release revision.
    Otherwise a repository-relative path or the resolve prefix of any revision is
    accepted as well.
    """
    prefix = resolve_prefix(metadata)
    if content_url.startswith(prefix):
        return _checked_path(content_url[len(prefix) :], content_url)
    if strict:
        raise CroissantLayoutError(f"contentUrl must start with {prefix!r}: {content_url}")
    resolve_root = f"{dataset_url(metadata)}/resolve/"
    if content_url.startswith(resolve_root):
        revision, separator, path = content_url[len(resolve_root) :].partition("/")
        if not revision or not separator:
            raise CroissantLayoutError(f"contentUrl has no revision and path: {content_url}")
        return _checked_path(path, content_url)
    return _checked_path(content_url, content_url)


def is_repository_object(item: dict[str, Any]) -> bool:
    formats = item.get("encodingFormat")
    if isinstance(formats, str):
        formats = [formats]
    return item.get("@type") == "cr:FileObject" and GIT_ENCODING_FORMAT in (formats or [])


def repository_ids(metadata: dict[str, Any]) -> list[str]:
    return [
        str(item.get("@id"))
        for item in metadata.get("distribution", [])
        if isinstance(item, dict) and is_repository_object(item)
    ]


def refresh_context(metadata: dict[str, Any]) -> bool:
    context = metadata.get("@context")
    if not isinstance(context, dict):
        raise SystemExit("Croissant metadata @context must be an object")

    changed = False
    for key, value in CONTEXT_1_1_ADDITIONS.items():
        if context.get(key) == value:
            continue
        context[key] = value
        changed = True
    return changed


def refresh_file_objects(metadata: dict[str, Any], root: Path = ROOT) -> bool:
    distribution = metadata.get("distribution")
    if not isinstance(distribution, list) or not distribution:
        raise SystemExit("Croissant metadata distribution must be a non-empty list")

    repositories = repository_ids(metadata)
    if len(repositories) > 1:
        raise SystemExit(f"Croissant distribution has several git repositories: {repositories}")
    changed = False
    file_objects = 0
    for item in distribution:
        if not isinstance(item, dict):
            raise SystemExit("Croissant distribution entries must be objects")
        item_id = item.get("@id", "<unknown>")
        if item.get("@type") == "cr:FileSet":
            if repositories and "containedIn" not in item:
                item["containedIn"] = {"@id": repositories[0]}
                changed = True
            continue
        if item.get("@type") != "cr:FileObject":
            continue
        if is_repository_object(item):
            url = dataset_url(metadata)
            if item.get("contentUrl") != url:
                item["contentUrl"] = url
                changed = True
            continue

        file_objects += 1
        content_url = item.get("contentUrl")
        if not isinstance(content_url, str) or not content_url:
            raise SystemExit(f"Croissant FileObject {item_id} is missing contentUrl")
        try:
            relative = content_path(metadata, content_url, strict=False)
        except CroissantLayoutError as error:
            raise SystemExit(f"Croissant FileObject {item_id}: {error}") from None
        canonical = content_url_for(metadata, relative)
        if content_url != canonical:
            item["contentUrl"] = canonical
            changed = True

        path = root / relative
        if not path.is_file():
            raise SystemExit(f"Croissant FileObject does not exist: {relative}")

        content_size = str(path.stat().st_size)
        sha256 = sha256_of(path)
        if item.get("contentSize") != content_size:
            item["contentSize"] = content_size
            changed = True
        if item.get("sha256") != sha256:
            item["sha256"] = sha256
            changed = True

    if file_objects == 0:
        raise SystemExit("Croissant distribution has no FileObject entries")
    return changed


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Refresh metadata/croissant.json generated fields."
    )
    parser.add_argument(
        "--date-modified",
        default=None,
        help="ISO date to write to dateModified. Defaults to today only when metadata changes.",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    metadata = load_metadata(METADATA_PATH)

    changed = False
    if metadata.get("conformsTo") != CROISSANT_SPEC:
        metadata["conformsTo"] = CROISSANT_SPEC
        changed = True

    changed = refresh_context(metadata) or changed
    try:
        changed = refresh_file_objects(metadata) or changed
    except CroissantLayoutError as error:
        raise SystemExit(str(error)) from None

    date_modified = args.date_modified
    if date_modified is None and changed:
        date_modified = dt.date.today().isoformat()

    if date_modified is not None:
        try:
            dt.date.fromisoformat(date_modified)
        except ValueError as exc:
            raise SystemExit(f"dateModified must be an ISO date: {date_modified}") from exc
        if metadata.get("dateModified") != date_modified:
            metadata["dateModified"] = date_modified
            changed = True

    if not changed:
        print(f"Croissant metadata already current: {METADATA_PATH.relative_to(ROOT)}")
        return

    METADATA_PATH.write_text(
        json.dumps(metadata, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    print(f"updated Croissant metadata: {METADATA_PATH.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
