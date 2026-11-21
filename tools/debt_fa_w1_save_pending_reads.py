#!/usr/bin/env python3
"""Collapse queued chunk reads to one record accessor ahead of removing edit retries."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "crates/sunrise-client/src"


def replace(path: Path, edits: list[tuple[str, str, int]]) -> None:
    text = path.read_text()
    for old, new, count in edits:
        if text.count(old) != count:
            raise SystemExit(f"{path}: expected {count} occurrences of {old!r}")
        text = text.replace(old, new)
    path.write_text(text)


replace(ROOT / "save.rs", [
    (", Record::tag_for_metadata)", ")", 3),
    (", Record::tag)", ")", 1),
    ("        read_pending: fn(&Record) -> Option<Arc<NamedTag>>,\n", "", 2),
    ("read_pending(&record)", "record.tag()", 1),
    ("(folder, x, z, read_pending)", "(folder, x, z)", 1),
])
replace(ROOT / "save/far_regions.rs", [("record.tag_for_metadata()", "record.tag()", 1)])
replace(ROOT / "save/record.rs", [(
    "    /// Metadata and far terrain do not consume a block entity's saved payload.\n"
    "    pub(super) fn tag_for_metadata(&self) -> Option<Arc<NamedTag>> {\n"
    "        if let Self::EditedChunk(record) = self {\n"
    "            return record.original.without_block_entity_edits();\n"
    "        }\n"
    "        self.tag()\n"
    "    }\n\n", "", 1,
)])
