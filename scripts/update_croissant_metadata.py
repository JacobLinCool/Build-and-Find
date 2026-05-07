#!/usr/bin/env python3
"""Refresh generated fields in metadata/croissant.json."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
METADATA_PATH = ROOT / "metadata" / "croissant.json"
CROISSANT_SPEC = "http://mlcommons.org/croissant/1.1"

CONTEXT_1_1_ADDITIONS = {
    "annotation": "cr:annotation",
    "excludes": "cr:excludes",
    "readLines": "cr:readLines",
    "sdVersion": "cr:sdVersion",
    "unArchive": "cr:unArchive",
    "value": "cr:value",
}


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


def refresh_file_objects(metadata: dict[str, Any]) -> bool:
    distribution = metadata.get("distribution")
    if not isinstance(distribution, list) or not distribution:
        raise SystemExit("Croissant metadata distribution must be a non-empty list")

    changed = False
    file_objects = 0
    for item in distribution:
        if not isinstance(item, dict):
            raise SystemExit("Croissant distribution entries must be objects")
        if item.get("@type") != "cr:FileObject":
            continue

        file_objects += 1
        content_url = item.get("contentUrl")
        if not isinstance(content_url, str) or not content_url:
            item_id = item.get("@id", "<unknown>")
            raise SystemExit(f"Croissant FileObject {item_id} is missing contentUrl")

        path = ROOT / content_url
        if not path.is_file():
            raise SystemExit(f"Croissant FileObject does not exist: {content_url}")

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
    changed = refresh_file_objects(metadata) or changed

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
