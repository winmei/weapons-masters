#!/usr/bin/env python3
"""Validate a Weapons Masters world export JSON file.

This script is intentionally small and dependency-free so it can run from Rider,
PowerShell, CI, or a Godot export hook.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


REQUIRED_TOP_LEVEL = {
    "schema_version",
    "map_id",
    "bounds",
    "colliders",
    "spawns",
    "mob_camps",
    "transitions",
    "zones",
}


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def require_id_list(document: dict, key: str) -> None:
    values = document.get(key)
    if not isinstance(values, list):
        fail(f"`{key}` must be a list")

    seen: set[str] = set()
    for index, item in enumerate(values):
        if not isinstance(item, dict):
            fail(f"`{key}[{index}]` must be an object")
        item_id = item.get("id")
        if not isinstance(item_id, str) or not item_id:
            fail(f"`{key}[{index}].id` must be a non-empty string")
        if item_id in seen:
            fail(f"duplicate id `{item_id}` in `{key}`")
        seen.add(item_id)


def main() -> None:
    if len(sys.argv) != 2:
        fail("usage: validate_world_export.py <map.world.json>")

    path = Path(sys.argv[1])
    if not path.exists():
        fail(f"file not found: {path}")

    try:
        document = json.loads(path.read_text(encoding="utf-8-sig"))
    except json.JSONDecodeError as error:
        fail(f"invalid JSON: {error}")

    if not isinstance(document, dict):
        fail("world export root must be an object")

    missing = sorted(REQUIRED_TOP_LEVEL - document.keys())
    if missing:
        fail(f"missing required keys: {', '.join(missing)}")

    for key in ("colliders", "spawns", "mob_camps", "transitions", "zones"):
        require_id_list(document, key)

    bounds = document["bounds"]
    if not isinstance(bounds, dict):
        fail("`bounds` must be an object")
    for key in ("min_x", "min_y", "max_x", "max_y"):
        if not isinstance(bounds.get(key), (int, float)):
            fail(f"`bounds.{key}` must be a number")
    if bounds["min_x"] >= bounds["max_x"] or bounds["min_y"] >= bounds["max_y"]:
        fail("bounds min values must be lower than max values")

    print(f"OK: {path}")


if __name__ == "__main__":
    main()
