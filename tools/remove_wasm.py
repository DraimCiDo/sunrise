#!/usr/bin/env python3
"""Apply the mechanical source-tree deletions for the native-only mod route."""

from pathlib import Path
import re
import subprocess


def main():
    root = Path(__file__).resolve().parents[1]
    subprocess.run(
        ["git", "rm", "-r", "--",
         "crates/sunrise-mod-wasm", "crates/sunrise-wasmtime",
         "crates/sunrise-mod-marks", "crates/sunrise-mod-points/src/guest.rs",
         "crates/sunrise-mod-points/tests/wire_loopback.rs"],
        cwd=root,
        check=True,
    )
    workspace = root / "Cargo.toml"
    source = workspace.read_text()
    source = re.sub(r'^    "crates/sunrise-(?:mod-wasm(?:/wire-loopback)?|wasmtime|mod-marks)",\n',
                    "", source, flags=re.M)
    source = re.sub(r'(?m)^# Fixture tests compile real wasm modules.*?(?=^\[profile.dev.package.sunrise-mod-api-gen\])',
                    "", source, flags=re.S)
    workspace.write_text(source)
    for crate in ("sunrise-mod-host", "sunrise-client", "sunrise-client-lib",
                  "sunrise-mod-dimension-game", "sunrise-mod-vocab"):
        manifest = root / "crates" / crate / "Cargo.toml"
        source = re.sub(r'^sunrise-mod-(?:wasm|marks) = .*\n', "", manifest.read_text(), flags=re.M)
        manifest.write_text(source)


if __name__ == "__main__":
    main()
