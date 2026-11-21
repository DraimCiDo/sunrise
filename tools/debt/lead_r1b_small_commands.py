#!/usr/bin/env python3
"""Replace command reply-list writes with scoped command registration calls."""
from pathlib import Path
import re
import subprocess

root = Path(__file__).resolve().parents[2]
paths = subprocess.check_output(
    ["git", "grep", "-l", "-E", r"Register(Client)?CommandsEvent<'_>", "--", "crates", "docs/modding"],
    cwd=root, text=True,
).splitlines()
for name in paths:
    path = root / name
    old = path.read_text()
    updated = re.sub(r"\bevent\s*\.commands\s*\.push\(", "event.register(", old)
    updated = re.sub(
        r"\bevent\s*\.commands\s*\.extend\(\[(.*?)\]\);",
        lambda match: f"for command in [{match[1]}] {{\n            event.register(command);\n        }}",
        updated,
        flags=re.S,
    )
    if updated != old:
        path.write_text(updated)
