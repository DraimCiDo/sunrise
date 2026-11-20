#!/usr/bin/env python3
"""One-off debt 23 codemod: give the engine capture planner its own edge type."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CAPTURE = ROOT / "crates/sunrise-client/src/api_text.rs"

source = CAPTURE.read_text(encoding="utf-8")
if "TextProfileProperty, TextRelation, TextStyle," not in source:
    raise SystemExit("expected the original engine text-relation import")
source = source.replace("TextProfileProperty, TextRelation, TextStyle,", "TextProfileProperty, TextStyle,")
source = source.replace("TextRelation", "CaptureEdge")
source = source.replace(
    "struct Node<'a> {",
    """#[derive(Clone, Copy)]
enum CaptureEdge {
    Child,
    Argument,
    Separator,
    Fallback,
    HoverText,
    HoverEntityName,
}
struct Node<'a> {""",
    1,
)
source = source.replace('        _ => unreachable!("component visitor supplied an unknown relation"),\n', "")
CAPTURE.write_text(source, encoding="utf-8")
