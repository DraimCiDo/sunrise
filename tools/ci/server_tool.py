#!/usr/bin/env python3
"""Build matrix and signed metadata for .github/workflows/server-tool.yml."""
import argparse
import datetime as dt
import json
import os
from pathlib import Path
import re
import sys


def matrix(game_build: Path) -> dict:
    """Every game build leg ships sunrise-server; use its platform names, targets and native runners.

    The separate musl job is a manual download, not an update target (update.rs's assets_for).
    """
    sys.path.insert(0, str(Path(__file__).with_name("vendor") / "pyyaml-6.0.3.zip"))
    import yaml

    jobs = yaml.load(game_build.read_text(), Loader=yaml.BaseLoader)["jobs"]
    legs = jobs["build"]["strategy"]["matrix"]["include"]
    include = []
    for leg in legs:
        match = re.fullmatch(r"sunrise-([a-z0-9_-]+)\.(?:tar\.gz|zip)", leg["asset"])
        if not match:
            raise SystemExit(f"not a game platform asset: {leg['asset']}")
        exe = leg.get("exe", "")
        platform = match[1]
        extension = "zip" if exe else "tar.gz"
        target = leg.get("target", "")
        # The shared main-stack startup check fails under Rosetta; build and start the small tool on native Intel.
        runner = jobs["macos-x64-intel"]["runs-on"] if target == "x86_64-apple-darwin" else leg["os"]
        include.append({"name": leg["name"], "os": runner, "target": target, "rustflags": leg.get("rustflags", ""),
                        "asset": f"server-tool-{platform}.{extension}", "exe": exe})
    return {"include": include}


def version_info(document: object, commit: str, commit_time: int) -> dict:
    """Refuse an archive from the wrong checkout or metadata an updater could not read."""
    date = dt.datetime.fromtimestamp(commit_time, dt.timezone.utc).isoformat()
    if not isinstance(document, dict) or document.keys() != {"commit", "date", "supports"}:
        raise ValueError("expected commit, date and supports")
    if document["commit"] != commit or document["date"] not in (date, date.replace("+00:00", "Z")):
        raise ValueError(f"expected the built commit {commit} at {date}")
    supports = document["supports"]
    if (not isinstance(supports, dict) or supports.keys() != {"min", "max"}
            or any(type(v) is not int for v in supports.values())
            or not 1 <= supports["min"] <= supports["max"] <= 2**32 - 1):
        raise ValueError("expected an inclusive range of dedicated launch contracts")
    return document


def check_assets(folder: Path, legs: dict, signed: bool) -> list[str]:
    """Require the complete release file set, and return the names the signature must cover."""
    payload = sorted([leg["asset"] for leg in legs["include"]] + ["server-tool.json"])
    expected = sorted(payload + (["SHA256SUMS", "SHA256SUMS.sig"] if signed else []))
    actual = sorted(path.name for path in folder.iterdir())
    if expected != actual or not all((folder / name).is_file() for name in expected):
        raise SystemExit(f"not releasing: expected {', '.join(expected)} in {folder}, found {', '.join(actual)}")
    return payload


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("matrix").add_argument("game_build", type=Path)
    info = commands.add_parser("info", help="validate --version --json on stdin, write release metadata to stdout")
    info.add_argument("commit")
    info.add_argument("commit_time", type=int)
    assets = commands.add_parser("assets", help="check assets against the MATRIX env var")
    assets.add_argument("folder", type=Path)
    assets.add_argument("--signed", action="store_true")
    args = parser.parse_args()
    if args.command == "matrix":
        print(json.dumps(matrix(args.game_build), separators=(",", ":")))
    elif args.command == "info":
        try:
            print(json.dumps(version_info(json.load(sys.stdin), args.commit, args.commit_time), indent=2))
        except ValueError as error:
            sys.exit(f"sunrise-server --version --json: {error}")
    else:
        print("\n".join(check_assets(args.folder, json.loads(os.environ["MATRIX"]), args.signed)))


if __name__ == "__main__":
    main()
