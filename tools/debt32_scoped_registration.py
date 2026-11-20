#!/usr/bin/env python3
"""Apply the mechanical parts of debt 32, one commit-sized slice at a time.

Preview with `python3 tools/debt32_scoped_registration.py component --dry-run`
(or `plain`). Apply only once the generator supports `registration_append`.
"""

import argparse
import difflib
import re
import sys
from pathlib import Path


VOCAB = "crates/sunrise-mod-vocab/src/hooks"
CLIENT = "crates/sunrise-client"
BOOK = "docs/modding/src/how-to/client/render-pipelines.md"
LOAD_STATE = "crates/sunrise-mod-api/src/load_state.rs"

# Hook name, side, old output field, NeoForge anchor.
HOOKS = {
    "component": (
        ("component_types", "both", "types", "RegisterEvent"),
    ),
    "plain": (
        ("create_skull_models", "client", "models", "EntityRenderersEvent.CreateSkullModels"),
        ("map_decoration_types", "both", "types", "RegisterEvent"),
        ("register_entity_spectator_shaders", "client", "shaders", "RegisterEntitySpectatorShadersEvent"),
        ("register_fluid_models", "client", "models", "RegisterFluidModelsEvent"),
        ("register_render_pipelines", "client", "pipelines", "RegisterRenderPipelinesEvent"),
        ("register_texture_atlases", "client", "atlases", "RegisterTextureAtlasesEvent"),
    ),
}

# Each count belongs to one listener/example body, in source order within the file.
LISTENERS = {
    "component_types": (
        ("crates/sunrise-mod-api-gen/dispatch-tests/src/fixtures/base.rs", (1,)),
    ),
    "create_skull_models": ((f"{CLIENT}/tests/it/mod_api_hooks.rs", (1,)),),
    "register_entity_spectator_shaders": (
        (f"{VOCAB}/register_entity_spectator_shaders.rs", (1,)),
        (f"{CLIENT}/src/game/modding/native_spectator_shader_tests.rs", (2,)),
    ),
    "register_fluid_models": (
        (f"{CLIENT}/src/game/modding/mods/mod.rs", (1, 1)),
        (f"{CLIENT}/src/game/world/packs/mod.rs", (1,)),
    ),
    "register_render_pipelines": (
        (f"{VOCAB}/register_render_pipelines.rs", (1,)),
        (BOOK, (1,)),
    ),
    "register_texture_atlases": (
        (f"{CLIENT}/src/game/interface/client_reload/texture_atlases.rs", (2,)),
    ),
}


def expect_count(actual, expected, description):
    if actual != expected:
        raise ValueError(f"{description}: expected {expected}, found {actual}")


def replace_once(source, before, after):
    expect_count(source.count(before), 1, f"occurrences of {before.strip()!r}")
    return source.replace(before, after)


def rewrite_hook(source, hook, side, field, neoforge):
    attributes = [f'side = "{side}"', 'contract = "collect"', f'out = "{field}"']
    if hook == "register_render_pipelines":
        attributes.append(
            'collect_validator = "crate::client::render_pipeline::validate_render_pipelines"'
        )
    attributes.extend(['context = "registration"', f'neoforge = "{neoforge}"'])
    declaration = f"\n    )]\n    fn {hook}();"
    before = "    #[hook(\n        " + ",\n        ".join(attributes) + declaration
    # These declarations all omit `version`, which defaults to 1.
    after = before.replace(
        f'out = "{field}",', f'out = "{field}",\n        registration_append = "register",'
    ).replace(declaration, ",\n        version = 2" + declaration)
    return replace_once(source, before, after)


def rewrite_listener_pushes(source, hook, field, counts):
    event_type = "".join(word.title() for word in hook.split("_")) + "Event"
    listener = re.compile(
        rf"(?m)^(?P<indent> *)fn {hook}\(\n"
        r"(?P=indent)    &self,\n"
        r"(?P=indent)    \w+: &(?:mod_api::)?RegistrationContext,\n"
        rf"(?P=indent)    event: &mut (?:mod_api::)?{event_type}<'_>,\n"
        r"(?P=indent)\) \{\n"
        r"(?P<body>[\s\S]*?)^(?P=indent)\}"
    )
    matches = list(listener.finditer(source))
    expect_count(len(matches), len(counts), f"{hook} listener bodies")

    old_field = rf"event\s*\.\s*{field}\b"
    push = re.compile(rf"(?m)^([ \t]*){old_field}\s*\.\s*push\b(?=\s*\()")
    for index in reversed(range(len(matches))):
        body = matches[index]
        start, end = body.span("body")
        updated, count = push.subn(r"\g<1>event.register", body["body"])
        expect_count(count, counts[index], f"{hook} listener {index + 1} pushes")
        if re.search(rf"\b{old_field}", updated):
            raise ValueError(f"{hook} listener {index + 1} still accesses event.{field}")
        source = source[:start] + updated + source[end:]
    return source


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("slice", choices=HOOKS)
    parser.add_argument("--dry-run", action="store_true", help="show the diff without writing")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    changes = {}

    def stage(file, rewrite, *params):
        try:
            if file in changes:
                before, source = changes[file]
            else:
                before = source = (root / file).read_bytes().decode("utf-8")
            changes[file] = before, rewrite(source, *params)
        except (OSError, ValueError) as error:
            parser.exit(1, f"{file}: {error}\n")

    for hook, side, field, neoforge in HOOKS[args.slice]:
        stage(f"{VOCAB}/{hook}.rs", rewrite_hook, hook, side, field, neoforge)
        for file, counts in LISTENERS.get(hook, ()):
            stage(file, rewrite_listener_pushes, hook, field, counts)

    if args.slice == "component":
        stage(
            LOAD_STATE,
            replace_once,
            "                *event.types = probe(cx);\n",
            "                for component_type in probe(cx) {\n"
            "                    event.register(component_type);\n"
            "                }\n",
        )
    else:
        # The optional and textured pipeline examples are standalone Rust snippets.
        before, source = changes[BOOK]
        updated, count = re.subn(
            r"(?m)^(```rust,ignore\n)event\s*\.\s*pipelines\s*\.\s*push\b(?=\s*\()",
            r"\g<1>event.register",
            source,
        )
        if count != 2 or re.search(r"\bevent\s*\.\s*pipelines\b", updated):
            parser.exit(1, f"{BOOK}: expected only two remaining pipeline snippet pushes\n")
        changes[BOOK] = before, updated

    # Check every preimage in the slice before writing any file.
    for file, (before, after) in sorted(changes.items()):
        if args.dry_run:
            sys.stdout.writelines(
                difflib.unified_diff(
                    before.splitlines(keepends=True), after.splitlines(keepends=True),
                    f"a/{file}", f"b/{file}",
                )
            )
        else:
            (root / file).write_bytes(after.encode("utf-8"))


if __name__ == "__main__":
    main()
