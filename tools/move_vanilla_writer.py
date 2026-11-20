#!/usr/bin/env python3
r"""Move the existing vanilla writer below protocol, rewriting its callers (#3637).

Replay from a clean checkout with `python3 tools/move_vanilla_writer.py`, then
format the changed Rust files and the new module:

    (git diff --name-only -z -- '*.rs'; printf '%s\0' crates/sunrise-core/src/vanilla_bytes.rs) \
        | xargs -0 rustfmt --edition 2024 --config skip_children=true
"""

from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parents[1]
PROTOCOL = ROOT / "crates/sunrise-protocol"
CORE = ROOT / "crates/sunrise-core/src"
BUFFER = PROTOCOL / "src/buf.rs"
DESTINATION = CORE / "vanilla_bytes.rs"
PARTS = r"Writer|write_varint|varint_size"
MODULE = r"(?:\b(?:sunrise_protocol|crate|super)::)*\bbuf::"

source = BUFFER.read_text()
start = source.index("/// An outgoing packet body.")
stop = source.index("/// A wire encoding for values", start)
assert not DESTINATION.exists(), DESTINATION
DESTINATION.write_text(
    "//! Primitive encodings for vanilla Minecraft packet bodies.\n\n"
    "use crate::nbt::{self, Tag};\n\n" + source[start:stop].rstrip() + "\n"
)
BUFFER.write_text(
    source[:start] + "use sunrise_core::vanilla_bytes::Writer;\n\n" + source[stop:]
)
lib = CORE / "lib.rs"
source = lib.read_text()
assert "pub mod position;" in source
lib.write_text(source.replace("pub mod position;", "pub mod position;\npub mod vanilla_bytes;"))


def rewrite_import(match):
    indent, tree = match.groups()
    moved = []

    def extract_group(group):
        rest = []
        for item in group[2].split(","):
            item = item.strip()
            if re.fullmatch(rf"(?:{PARTS})(?:\s+as\s+\w+)?", item):
                moved.append(item)
            elif item:
                rest.append(item)
        return group[1] + "{" + ", ".join(rest) + "}" if rest else ""

    tree = re.sub(rf"({MODULE})\{{([^{{}}]*)\}}", extract_group, tree)

    def extract_leaf(leaf):
        moved.append(leaf[1])
        return ""

    tree = re.sub(rf"{MODULE}((?:{PARTS})\b(?:\s+as\s+\w+)?)", extract_leaf, tree)
    if not moved:
        return match[0]
    # A removed leaf can leave an extra comma or an empty enclosing path.
    tree = re.sub(r"([,{])\s*,", r"\1", tree)
    tree = re.sub(r"(?:sunrise_protocol|crate|super)::\{\s*\}", "", tree).strip()
    if tree:
        prefix = f"{indent}use {tree};\n"
    else:
        prefix = ""
    picks = moved[0] if len(moved) == 1 else "{" + ", ".join(moved) + "}"
    return f"{prefix}{indent}use sunrise_core::vanilla_bytes::{picks};"


tracked = subprocess.check_output(
    ["git", "ls-files", "-z", "--", "*.rs"], cwd=ROOT,
).decode().split("\0")
for file in filter(None, tracked):
    path = ROOT / file
    source = path.read_text()
    text = re.sub(r"(?m)^([ \t]*)use ([^;]+);", rewrite_import, source)
    if path.is_relative_to(PROTOCOL):
        root = r"(?:\$?crate|sunrise_protocol|self)"
    else:
        root = r"sunrise_protocol"
    text = re.sub(rf"{root}::buf::({PARTS})\b", r"::sunrise_core::vanilla_bytes::\1", text)
    if text != source:
        path.write_text(text)
