#!/usr/bin/env python3
"""Construct fixture states directly before removing the BlockState transport twin."""

import re
import subprocess
from pathlib import Path

VOCAB = "crates/sunrise-mod-vocab/src"
SCOPES = (
    "crates/sunrise-client/src/game",
    "crates/sunrise-client-render/src/particles/native.rs",
    "crates/sunrise-mod-api/src/parts/blocks_fixtures",
    f"{VOCAB}/block/block_state_range.rs",
    f"{VOCAB}/block/block_states.rs",
    f"{VOCAB}/parts/shared.rs",
    f"{VOCAB}/parts/shared/worldgen_tests.rs",
    f"{VOCAB}/testing.rs",
    f"{VOCAB}/wire/wire_layout_tests.rs",
)
# The range iterator's grouped ordinal arithmetic nests one method call.
ARGS = r"((?:[^()]|\((?:[^()]|\([^()]*\))*\))*)"
RECORD = rf"(?:mod_api::|crate::)?BlockStateRecord::new\({ARGS}\)"
FROM_RECORD = re.compile(r"\b(?:mod_api::|crate::)?BlockState::from\(\s*" + RECORD + r"\s*\)")
INTO_STATE = re.compile(r"\b" + RECORD + r"\.into\(\)")

CONSTRUCTOR = """/// Construct a state without looking up the key or ordinal in block metadata.
#[doc(hidden)]
pub const fn block_state_from_ordinal(block: BlockKey, ordinal: u16) -> BlockState {
    BlockState {
        block,
        state: ordinal,
    }
}

"""
EXPORT = """
pub mod __private {
    pub use crate::block::block_state::block_state_from_ordinal;
}
"""


def main():
    root = Path(__file__).resolve().parents[1]
    fixtures = subprocess.run(
        ["git", "grep", "-lz", "-F", "BlockStateRecord::new", "--", *SCOPES],
        cwd=root,
        capture_output=True,
    )
    if fixtures.returncode != 1:  # No matches after the script has run.
        fixtures.check_returncode()

    edits = {}
    for name in filter(None, fixtures.stdout.decode().split("\0")):
        path = root / name
        old = path.read_text()
        if name.startswith("crates/sunrise-mod-api/"):
            prefix = "mod_vocab"
        elif name.startswith(f"{VOCAB}/"):
            prefix = "crate"
        else:
            prefix = "mod_api"
        call = rf"{prefix}::__private::block_state_from_ordinal(\1)"
        new = INTO_STATE.sub(call, FROM_RECORD.sub(call, old))
        if "BlockStateRecord::new" in new:
            raise SystemExit(f"Unmigrated constructor in {name}")
        if new.count("BlockStateRecord") == 1:
            # Only the import of the twin remains. Keep block_states.rs's import
            # for the metadata/backend implementations migrated in the next step.
            new = new.replace("BlockState, BlockStateRecord, Compound", "Compound")
            new = new.replace("{BlockStateRecord, HostBlockStates}", "HostBlockStates")
            new = new.replace(", BlockStateRecord}", "}").replace("BlockStateRecord, ", "")
        edits[path] = new

    path = root / VOCAB / "block/block_state.rs"
    old = path.read_text()
    if "pub const fn block_state_from_ordinal(" not in old:
        anchor = "impl From<BlockStateRecord> for BlockState {"
        if anchor not in old:
            raise SystemExit(f"Missing constructor insertion point in {path}")
        edits[path] = old.replace(anchor, CONSTRUCTOR + anchor, 1)
    path = root / VOCAB / "exports/block_state.rs"
    old = path.read_text()
    if EXPORT not in old:
        edits[path] = old + EXPORT

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
