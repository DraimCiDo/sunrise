#!/usr/bin/env python3
"""Apply the mechanical, point-only deletions and move the two shared modules.

Run on the parent of this commit from the repository root. This script refuses
missing source paths or existing destinations instead of deleting or overwriting
anything on a different source layout.
"""

from pathlib import Path
import subprocess


MOVES = {
    "crates/sunrise-engine-slots/src/platform.rs":
        "crates/sunrise-mod-vocab/src/__private/platform.rs",
    "crates/sunrise-mod-point-shape/src/build_data.rs":
        "crates/sunrise-mod-surface/src/build_data.rs",
}
DELETE = (
    "crates/sunrise-engine-slots",
    "crates/sunrise-mod-point-emit",
    "crates/sunrise-mod-point-shape",
    "crates/sunrise-mod-points",
    "crates/sunrise-mod-api-gen/src/points.rs",
    "crates/sunrise-mod-api-gen/native-point-types.txt",
    "crates/sunrise-mod-api-macros/src/point.rs",
    "crates/sunrise-mod-vocab/src/__private/points.rs",
    "crates/sunrise-mod-package/src/manifest/inject.rs",
    "crates/sunrise-native-build/src/inject.rs",
    "crates/sunrise-native-build/src/slots.rs",
    "crates/sunrise-native-build/src/slots",
    "crates/cargo-sunrise-mod/src/inject.rs",
    "crates/cargo-sunrise-mod/src/inject_sync.rs",
    "crates/cargo-sunrise-mod/src/inject_update.rs",
)


def main():
    for source in (*MOVES, *DELETE):
        if not Path(source).exists():
            raise SystemExit(f"missing source: {source}")
    for target in MOVES.values():
        if Path(target).exists():
            raise SystemExit(f"destination already exists: {target}")
    # Do not remove private fixtures or build outputs inside the old crates.
    tracked = subprocess.check_output(["git", "ls-files", "-z", "--", *DELETE])
    for source, target in MOVES.items():
        Path(source).rename(target)
    for source in tracked.decode().split("\0"):
        if source and source not in MOVES:
            Path(source).unlink()


if __name__ == "__main__":
    main()
