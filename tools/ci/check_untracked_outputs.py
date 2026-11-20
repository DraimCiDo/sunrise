#!/usr/bin/env python3
"""Generated files that must never be committed: a source tree built from one would be stale or wrong.

- `crates/sunrise-engine-slots/points-catalog.json`: the point catalog `sunrise-stage` writes into a staged copy. The
  point build scripts refuse it in a tree without its stage stamp; committing it would break every checkout.
- `crates/sunrise-mod-wasm/tests/fixture/points.json`: the wasm fixture's catalog, written by the
  `listener-fixture-catalog` example from `crates/sunrise-engine-slots/fixture-points.json` where the fixture builds.
"""
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUTPUTS = ("crates/sunrise-engine-slots/points-catalog.json", "crates/sunrise-mod-wasm/tests/fixture/points.json")


def tracked(root=ROOT, outputs=OUTPUTS):
    listed = subprocess.run(["git", "ls-files", "-z", "--", *outputs], cwd=root, capture_output=True, check=True)
    return sorted(filter(None, listed.stdout.decode().split("\0")))


def main(root=ROOT):
    found = tracked(root)
    for path in found:
        print(f"::error file={path}::{path} is a build output; remove it from Git (git rm --cached {path})")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
