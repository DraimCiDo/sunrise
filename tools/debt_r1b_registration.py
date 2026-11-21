#!/usr/bin/env python3
"""Convert registration-context vector outputs to the existing scoped event setter."""

import re
import subprocess
from pathlib import Path

HOOKS = dict(
    register_block_state_models="definitions",
    register_debug_entries="entries",
    register_debug_renderers="renderers",
    register_dimension_transition_screen="screens",
    register_item_models="models",
    register_item_tint_sources="sources",
    register_json_animation_types="types",
    register_model_loaders="loaders",
    register_preset_editors="editors",
    register_range_select_item_model_property="properties",
    register_select_item_model_property="properties",
    register_sprite_sources="sources",
)
DECLS = Path("crates/sunrise-mod-vocab/src/hooks")
paths = subprocess.check_output(
    ["git", "grep", "-lzE", r"fn (" + "|".join(HOOKS) + r")\(", "--", "*.rs", "docs/modding/*.md"]
).decode().strip("\0").split("\0")
for filename in paths:
    path = Path(filename)
    source = path.read_text()
    if path.parent == DECLS and path.stem in HOOKS:
        out = f'out = "{HOOKS[path.stem]}",'
        assert source.count(out) == 1, path
        source = source.replace(out, f'{out}\n        registration_append = "register",')
        source = source.replace('        owned_answer_batch = true,\n', '')
        source, n = re.subn(r'\bversion = (\d+)', lambda m: f'version = {int(m[1]) + 1}', source)
        if not n:
            source = source.replace('        contract = "collect",', '        version = 2,\n        contract = "collect",')

    for hook, field in HOOKS.items():
        # Match the complete listener definition (not engine functions with the same name).
        listener = rf'(?ms)^(?P<indent> *)fn {hook}\((?P<args>[^{{;]*?\))\s*\{{\n(?P<body>.*?)^(?P=indent)\}}'

        def rewrite(m):
            if not re.search(rf'\bevent: &mut (?:mod_api::)?{"".join(w.capitalize() for w in hook.split("_"))}Event\b', m['args']):
                return m[0]
            access = rf'event\s*\.{field}\s*\.'
            body = re.sub(access + r'push\b', 'event.register', m['body'])
            body = re.sub(rf'(?m)^ +assert!\({access}is_empty\(\)[^;]*\);\n', '', body)
            body = re.sub(
                rf'(?m)^( +){access}extend\(self\.0\.iter\(\)\.map\(\|(&?id)\| (\w+)\(id\)\)\);',
                r'\1for \2 in self.0 {\n\1    event.register(\3(id));\n\1}',
                body,
            )
            assert not re.search(access, body), (path, hook, body)
            start, end = m.span('body')
            return source[m.start():start] + body + source[end:m.end()]

        source = re.sub(listener, rewrite, source)
    path.write_text(source)
