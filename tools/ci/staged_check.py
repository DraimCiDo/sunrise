#!/usr/bin/env python3
"""Check what only a staged copy compiles: the game's point sites and the code that names a point.

A source tree's build has no point catalog (`sunrise-stage` writes it into the staged copy the prebuilt game builds
from), so the points' records, dispatch and `#[point]` sites compile only there. This stages HEAD's tracked files, as
`tools/prebuilt_game.py build` does, then type-checks the game, the wasm host and the modding book's examples
(`docs/modding/examples`, their own workspace) in that copy. The gate runs it; the daily's prebuilt game builds the
same copy in full.

    python3 tools/ci/staged_check.py TARGET_DIR [--stage-tool SUNRISE_STAGE] [--examples-only] [--test-examples]
"""
import argparse
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import prebuilt_game  # noqa: E402


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("target", type=Path, help="a folder this check owns: the staged copy and its build go in it")
    parser.add_argument("--stage-tool", type=Path, help="a built sunrise-stage, instead of building one")
    parser.add_argument("--examples-only", action="store_true", help="check only the modding book's examples")
    parser.add_argument("--test-examples", action="store_true", help="run the examples' tests, not only check them")
    args = parser.parse_args(argv)
    target = args.target.resolve()
    staged = prebuilt_game.stage(target, args.stage_tool)
    jobs = [arg for arg in os.environ.get("JOBS", "").split() if arg]
    env = {**os.environ, "CARGO_TARGET_DIR": os.environ.get("CARGO_TARGET_DIR", str(target / "build"))}
    commands = []
    if not args.examples_only:
        commands.append(["cargo", "check", *jobs, "--locked", "-p", "sunrise-client", "-p", "sunrise-mod-wasm"])
    examples = ["--locked", "--manifest-path", "docs/modding/examples/Cargo.toml"]
    commands.append(["cargo", "test" if args.test_examples else "check", *jobs, *examples])
    for command in commands:
        print("+", " ".join(command), flush=True)
        if subprocess.run(command, cwd=staged, env=env).returncode:
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
