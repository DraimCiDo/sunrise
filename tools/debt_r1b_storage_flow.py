#!/usr/bin/env python3
"""Carry the visitor's ControlFlow through every ItemStorage adapter and fixture."""

import re
from pathlib import Path

VOCAB = Path("crates/sunrise-mod-vocab/src")
CLIENT = Path("crates/sunrise-client/src/game")

storage = VOCAB / "item/storage.rs"
for path, method, flow, error, count in (
    (storage, "visit_entries", "ControlFlow", "StorageError", 5),
    (VOCAB / "host/items.rs", "container_visit_entries", "std::ops::ControlFlow", "LevelReadError", 1),
    (VOCAB / "testing.rs", "container_visit_entries", "std::ops::ControlFlow", "crate::LevelReadError", 1),
    (CLIENT / "blocks/machines/native_items.rs", "container_visit_entries", "std::ops::ControlFlow", "LevelReadError", 1),
):
    source = path.read_text()
    signature = rf"(\bfn {method}\(.*?\) -> Result<)\(\)(, {re.escape(error)}>)"
    source, matches = re.subn(signature, rf"\g<1>{flow}<()>\2", source, flags=re.S)
    assert matches == count, path

    def preserve_flow(match):
        body = match[0].replace("Ok(())", f"Ok({flow}::Continue(()))")
        body = re.sub(
            r"(?m)^( +)if (visitor\([^\n]*\))\.is_break\(\) \{\n +break;",
            r"\1let flow = \2;\n\1if flow.is_break() {\n\1    return Ok(flow);",
            body,
        )
        return body.replace(
            "if amount != 0 && visitor(resource, amount).is_break() {\n                    break;\n                }",
            "if amount != 0 {\n                    let flow = visitor(resource, amount);\n"
            "                    if flow.is_break() {\n                        return Ok(flow);\n                    }\n                }",
        )

    source = re.sub(
        rf"(?ms)^( +)[^\n]*\bfn {method}\(\n(?:(?!\) -> Result<).)*\) -> Result<[^\n;]+ \{{\n.*?^\1\}}",
        preserve_flow,
        source,
    )
    path.write_text(source)

for path in (storage, CLIENT / "modding/native_block_entity_item_tests.rs"):
    source = path.read_text()
    source = re.sub(
        r"(?m)^([ ]*)((?:storage|bottom|from)(?:\n\s*)?\.visit_entries\()",
        r"\1let _ = \2",
        source,
    )
    path.write_text(source)
