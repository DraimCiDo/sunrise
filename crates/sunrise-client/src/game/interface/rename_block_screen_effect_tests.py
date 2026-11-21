#!/usr/bin/env python3
"""Rename the overlay test module and every tracked test selector for it."""
from pathlib import Path
import subprocess

old = 'block_screen_effect_draw_tests'
new = 'block_screen_effect_tests'
root = Path(__file__).resolve().parents[5]
paths = subprocess.check_output(
    ['git', 'grep', '-z', '-l', old, '--', 'crates', 'tools'], cwd=root, text=True
)
for name in paths.rstrip('\0').split('\0'):
    path = root / name
    path.write_text(path.read_text().replace(old, new))
    if path.stem == old:
        path.rename(path.with_stem(new))
