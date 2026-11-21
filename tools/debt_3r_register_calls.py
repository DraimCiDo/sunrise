#!/usr/bin/env python3
"""Migrate straightforward unsynced register_attachment_type("ns:name", false) calls
to AttachmentDescriptor::new("ns:name", || ()) for debt 3r step 2.

Synced (`true`) and dynamic-id calls are handled separately by hand so each carries its
explicit sync codec or static id list.
"""
from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
PATTERN = re.compile(r'\.register_attachment_type\(\s*("[^"]+")\s*,\s*false\s*\)')


def migrate_file(path: pathlib.Path, prefix: str) -> int:
    text = path.read_text(encoding="utf-8")
    new_text, count = PATTERN.subn(
        rf".register_attachment_type({prefix}AttachmentDescriptor::new(\1, || ()))",
        text,
    )
    if count:
        path.write_text(new_text, encoding="utf-8")
    return count


def main() -> int:
    changed = 0
    targets = [
        (ROOT / "crates/sunrise-mod-api/src/parts", "crate::"),
        (ROOT / "crates/sunrise-client/src", "mod_api::"),
    ]
    skip = {ROOT / "crates/sunrise-mod-api/src/parts/attachment_types.rs"}
    for base, prefix in targets:
        for path in sorted(base.rglob("*.rs")):
            if path in skip:
                continue
            count = migrate_file(path, prefix)
            if count:
                rel = path.relative_to(ROOT)
                print(f"{rel}: {count}")
                changed += count
    print(f"total migrated: {changed}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
