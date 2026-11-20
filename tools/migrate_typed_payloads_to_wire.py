#!/usr/bin/env python3
"""Migrate the typed registrations and callers; remove the separate codec (#3637)."""

from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
VOCAB = ROOT / "crates/sunrise-mod-vocab/src"
MACROS = ROOT / "crates/sunrise-mod-api-macros/src"

for stem in (
    "payload_budget", "payload_budget_tests", "payload_codec", "payload_codec_tests",
    "payload_schema", "payload_schema_tests",
):
    (VOCAB / "content" / f"{stem}.rs").unlink()
(VOCAB / "exports/payload_codec.rs").unlink()
(MACROS / "payload.rs").unlink()

modules = VOCAB / "content/mod.rs"
modules.write_text(re.sub(r"^pub\(crate\) mod payload_\w+;\n", "", modules.read_text(), flags=re.M))
lib = MACROS / "lib.rs"
source = lib.read_text().replace("mod payload;\n", "")
start = source.index("/// Derive a typed payload")
stop = source.index("/// Derive the canonical", start)
lib.write_text(source[:start] + source[stop:])

for name in (
    "crates/sunrise-mod-vocab/src/payload/typed.rs",
    "crates/sunrise-mod-api/src/parts/payloads/typed_tests.rs",
    "crates/sunrise-mod-api/src/parts/payloads/group_tests.rs",
    "crates/sunrise-client/src/raw_payloads/typed_schema_tests.rs",
    "crates/sunrise-client/src/game/modding/payload_transport/typed_tests.rs",
    "docs/modding/src/how-to/client/networking.md",
):
    path = ROOT / name
    source = path.read_text().replace("PayloadCodec", "Wire")
    source = source.replace("VarInt, ", "")
    source = re.sub(r"VarInt\(([^()]*)\)", r"\1", source)
    source = re.sub(r"\bVarInt\b", "i32", source)
    path.write_text(source)
