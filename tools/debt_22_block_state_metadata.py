#!/usr/bin/env python3
"""Retype the BlockState metadata boundary's backend, table view and test double."""

import subprocess
from pathlib import Path

VOCAB = "crates/sunrise-mod-vocab/src"
HOST = f"{VOCAB}/host/block_states.rs"
SERVICE = f"{VOCAB}/block/block_states.rs"
TABLES = "crates/sunrise-client/src/game/modding/native_block_tables.rs"
TEST = "crates/sunrise-mod-api/src/parts/registration_block_metadata_tests.rs"
COUNTS = {HOST: 9, SERVICE: 14, TABLES: 11, TEST: 6}


def main():
    root = Path(__file__).resolve().parents[1]
    edits = {}
    for name, count in COUNTS.items():
        path = root / name
        old = path.read_text()
        found = old.count("BlockStateRecord")
        if not found:
            continue
        if found != count:
            raise SystemExit(f"{name}: expected {count} record references, found {found}")
        source = old
        if name == SERVICE:
            source = source.replace("BlockStateRecord, Host", "Host")
            source = source.replace("(&state).into()", "state")
        if name == TABLES:
            source = source.replace("mod_api::BlockState::from(state)", "state")
        if name == TEST:
            source = source.replace(
                "BlockStateRecord::new(", "crate::__private::block_state_from_ordinal("
            )
            source = source.replace("off.state", "off.ordinal()").replace(
                "on.state", "on.ordinal()"
            )
        edits[path] = source.replace("BlockStateRecord", "BlockState")

    for path, source in edits.items():
        path.write_text(source)
    if edits:
        subprocess.run(
            ["rustfmt", "--edition", "2024", "--config", "skip_children=true", *edits],
            cwd=root,
            check=True,
        )


if __name__ == "__main__":
    main()
