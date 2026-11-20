#!/usr/bin/env python3
"""Replay the mechanical steps for typed attachment registration (debt item 3).

Run `imports` first, then `receivers` to make Part state instance-based using
the existing AttachmentType. Run `registrations` when introducing
AttachmentDescriptor/AttachmentKey; it leaves work on the typed consumers
to their own pass. Use --dry-run to inspect a pass.
"""

import argparse
import difflib
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WAVE3 = Path("/box/shared/sunrise/debt-1.0/wave3-files.txt")
GAME = "crates/sunrise-client/src/game/"
API_PARTS = "crates/sunrise-mod-api/src/parts/"
VOCAB = "crates/sunrise-mod-vocab/src/"
RUNTIME = "sunrise_mod_runtime::attachments"


class Changes:
    def __init__(self, protected):
        self.protected = protected
        self.original = {}
        self.pending = {}

    def source(self, path):
        if path not in self.original:
            self.original[path] = (ROOT / path).read_text(encoding="utf-8")
        return self.pending.get(path, self.original[path])

    def expect(self, path, text, *, count=1):
        actual = self.source(path).count(text)
        if actual != count:
            raise ValueError(f"{path}: expected {count} copies of {text!r}, found {actual}")

    def replace(self, path, old, new, *, count=1):
        target = (ROOT / path).resolve().relative_to(ROOT).as_posix()
        if path in self.protected or target in self.protected:
            raise ValueError(f"{path}: protected by wave3-files.txt")
        self.expect(path, old, count=count)
        self.pending[path] = self.source(path).replace(old, new)


def imports(changes):
    # Engine conversion fixture: both markers are used on concrete Game, not via a mod Host.
    conversion = GAME + "mobs/creatures/conversion_identity.rs"
    changes.replace(conversion, "    use mod_api::{AttachmentType, Holder};\n", "")
    changes.replace(
        conversion,
        f"    use {RUNTIME}::{{AttachmentCodec, AttachmentError}};",
        f"    use {RUNTIME}::{{\n        AttachmentCodec, AttachmentError, AttachmentType, Holder,\n    }};",
    )

    # These workloads measure the direct Game storage path, without a mod or a registrar.
    for path, old_import, key in (
        (
            "modding/attachment_instruction_bench.rs",
            "use sunrise_core::{geometry::Vec3, position::ChunkPos};",
            "VALUE",
        ),
        ("runtime/hot_paths/mods.rs", "use sunrise_client::Result;", "COUNTER"),
    ):
        path = GAME + path
        changes.replace(path, old_import, f"{old_import}\nuse {RUNTIME}::AttachmentType;")
        changes.replace(
            path,
            f"static {key}: mod_api::AttachmentType<u32> = mod_api::AttachmentType {{",
            f"static {key}: AttachmentType<u32> = AttachmentType {{",
        )

    # Mixed fixtures are not bulk import edits: raw map/index seeding can keep its
    # engine descriptor; host calls and registrations need separate review.
    for path, markers in {
        # The local map seed is raw; the later VALUE declarations drive Game's holder/residency path.
        "entities/player_residency_tests.rs": (
            "VALUE.set(attachments, Holder::Level, 7)",
            "VALUE.set(game, mod_api::Holder::Entity(id), 73)",
        ),
        # The same VALUE seeds a raw map and writes through a transaction host.
        "modding/attachment_index/block_entity_tests.rs": (
            "VALUE.set(&mut map, Holder::Level, value)",
            "VALUE.set(host, Holder::BlockEntity([1, 64, 1]), 9)",
        ),
        # Raw index snapshots and the invalid-policy probe are separate from host transaction writes.
        "modding/attachment_index/painting_tests.rs": (
            "let bad = AttachmentType {",
            "before[0].entry.read(&VALUE), Ok(&0));\n            VALUE.set(host, holder, 10)",
        ),
        # The guest's map is seeded directly; later writes go through ServerHost.
        "modding/attachment_index/player_tests.rs": (
            ".set(guests.attachments_mut(12).unwrap(), Holder::Level, 31)",
            "VALUE.set(host, Holder::Player(12), 7)",
        ),
        # The ENERGY fixture tests dyn ServerHost (including the swapped guest state), not a raw map.
        "runtime/transactions.rs": (
            "const ENERGY: AttachmentType<i32> = AttachmentType {",
            "|host: &mut dyn ServerHost, holder| ENERGY.get_existing(host, holder)",
        ),
        # VALUE/OTHER reach raw player maps and index rows, and ServerHost for rollback.
        "runtime/transactions/player_tests.rs": (
            "VALUE.set(map, Holder::Level, value)",
            "OTHER.remove(host, Holder::Player(112))",
        ),
        # The raw VALUE transfer fixture is distinct from the registered natural_tree STATE.
        "world/server_travel.rs": (
            "VALUE.set(&mut map, Holder::Entity(owner.id()), 17)",
            "STATE.set(host, event.holder, 41)",
        ),
    }.items():
        for marker in markers:
            changes.expect(GAME + path, marker)


def fixture_descriptor(changes, path, symbol, codec_scope=""):
    """Share real defaults/codecs with consumers whose typed signatures have not changed yet."""
    pattern = (
        rf"^(?P<indent> *)(?:pub(?:\(super\))? )?(?:const|static) {symbol}: "
        r"(?:mod_api::)?AttachmentType<(?P<type>[^\n]+)> = (?:mod_api::)?AttachmentType \{\n"
        r"(?P<body>.*?)^(?P=indent)\};"
    )
    matches = list(re.finditer(pattern, changes.source(path), re.MULTILINE | re.DOTALL))
    if len(matches) != 1:
        raise ValueError(f"{path}: expected exactly one {symbol} initializer")
    match = matches[0]
    body, indent = match["body"], match["indent"]
    name = re.search(r'namespace: "([^"]+)",\n\s+name: "([^"]+)",', body)
    if name is None:
        raise ValueError(f"{path}: missing namespaced id on {symbol}")
    id = ":".join(name.groups())
    descriptor = f'mod_api::AttachmentDescriptor::new("{id}", {symbol}.default)'
    codec = re.search(r"codec: Some\(((?:mod_api::)?AttachmentCodec \{.*?\})\),", body, re.S)
    if codec:
        expr = codec[1]
        # Keep the fixture's original comments and attributes on its raw type, not on the codec.
        declaration = f"{indent}{codec_scope}const {symbol}_CODEC: mod_api::AttachmentCodec<{match['type']}> = {expr};\n\n"
        start = match.start()
        source = changes.source(path)
        while start and source[:start].splitlines()[-1].lstrip().startswith(("///", "#[")):
            start = source.rfind("\n", 0, start - 1) + 1
        changes.replace(path, source[start:match.end()], declaration + source[start:match.start()] + match[0].replace(expr, f"{symbol}_CODEC"))
        descriptor += f".serialize({symbol}_CODEC)"
    elif "codec: None," not in body:
        raise ValueError(f"{path}: unrecognized codec on {symbol}")
    if "copy_on_death: true," in body:
        descriptor += ".copy_on_death()"
    if "sync: true," in body:
        descriptor += ".sync()"
    return id, descriptor


def register_call(changes, path, id, descriptor, *, count=1):
    pattern = rf'\.register_attachment_type\(\s*"{re.escape(id)}", (?:true|false)\s*\)'
    matches = list(re.finditer(pattern, changes.source(path)))
    if len(matches) != count or len({m[0] for m in matches}) != 1:
        raise ValueError(f"{path}: unexpected old registration calls for {id}: {matches}")
    changes.replace(path, matches[0][0], f".register_attachment_type({descriptor})", count=count)


def registrations(changes):
    for base, path, symbols in (
        (GAME, "entities/general/tick_tests.rs", ("COUNTER",)),
        (GAME, "entities/passengers.rs", ("VEHICLE_VALUE",)),
        (GAME, "mobs/creatures/borrowed_panic_tests.rs", ("VALUE",)),
        (GAME, "modding/attachment_sync/transport_tests.rs", ("VALUE", "BLOB")),
        (GAME, "modding/block_entity_part_state_tests.rs", ("PROGRESS",)),
        (GAME, "modding/entity_initialization/tests.rs", ("VALUE", "SAVED")),
        (GAME, "modding/entity_part_state_tests.rs", ("CHARGE",)),
        (GAME, "modding/native_general_renderers_tests.rs", ("FUEL",)),
        (GAME, "modding/native_general_renderers_tests/fixture.rs", ("FUEL",)),
        (GAME, "modding/original_player.rs", ("VALUE", "RETAINED")),
        (GAME, "native_hooks/shared/item_fall_on_tests.rs", ("LOAN_VALUE",)),
        (GAME, "runtime/transaction_entities_tests.rs", ("MARK",)),
        (GAME, "world/server_travel.rs", ("STATE",)),
        (API_PARTS, "block_entities.rs", ("OWNERS_PROGRESS", "B_PROGRESS")),
        (API_PARTS, "owned_namespaces.rs", ("SESSION",)),
    ):
        for symbol in symbols:
            value_transport = path.endswith("transport_tests.rs") and symbol == "VALUE"
            id, expression = fixture_descriptor(changes, base + path, symbol, "pub(super) " if value_transport else "")
            count = {"vehicle_boundary:value": 2, "constructing:value": 3}.get(id, 1)
            register_call(changes, base + path, id, expression, count=count)
            if value_transport:
                snapshot = GAME + "modding/attachment_sync/general_snapshot_tests.rs"
                changes.replace(snapshot, "{VALUE, store}", "{VALUE, VALUE_CODEC, store}")
                register_call(changes, snapshot, id, expression)

    register_call(changes, GAME + "modding/original_player.rs", "clone:saved",
                  'mod_api::AttachmentDescriptor::new("clone:saved", SAVED.default).serialize(RETAINED_CODEC)')

    fixture = GAME + "modding/block_entity_part_state_tests/fixture.rs"
    calls = [fixture_descriptor(changes, fixture, symbol)[1] for symbol in ("COUNT", "MARK")]
    changes.replace(
        fixture,
        '    for name in ["part_state_count", "part_state_mark"] {\n'
        '        registrar\n'
        '            .register_attachment_type(&format!("part_state_fixture:{name}"), false)\n'
        '            .unwrap();\n'
        '    }',
        "\n".join(f"    registrar.register_attachment_type({call}).unwrap();" for call in calls),
    )
    changes.replace(
        API_PARTS + "shared_fixtures/fixturecontent.rs",
        "r.register_attachment_type(id, false).unwrap();",
        "r.register_attachment_type(crate::AttachmentDescriptor::new(id, || 0u32)).unwrap();",
    )

    # Policy/ownership tests intentionally use the same names with different policies.
    fixture = API_PARTS + "attachment_types.rs"
    changes.replace(fixture, "    use super::*;\n    struct Store", '''    use super::*;
    const MANA_CODEC: crate::AttachmentCodec<u32> = crate::AttachmentCodec {
        encode: |n| Ok([("n".into(), crate::Tag::Long(i64::from(*n)))].into()),
        decode: |saved| match saved.get(&"n".into()) {
            Some(crate::Tag::Long(n)) => u32::try_from(*n).map_err(|_| crate::AttachmentError::Decode),
            _ => Err(crate::AttachmentError::Decode),
        },
    };
    struct Store''')
    for indent, sync in ((16, False), (12, True)):
        expression = 'crate::AttachmentDescriptor::new("a:mana", || 37u32)'
        if sync:
            expression += ".serialize(MANA_CODEC).sync()"
        prefix = "\n" + " " * indent + "registrar.register_attachment_type("
        changes.replace(fixture, prefix + f'"a:mana", {str(sync).lower()}).unwrap();', prefix + expression + ").unwrap();")
    for caller, id, sync, count in (
        ("registrar", "a:mana", False, 2), ("bad", "test_pkg:session", False, 1),
        ("bad", "test_pkg:mana", True, 1), ("good", "test_pkg:mana", False, 1),
        ("registrar", "test_pkg:mana", True, 1), ("registrar", "a:mana", True, 1),
        ("conflict", "test_pkg:mana", True, 1), ("conflict", "a:mana", False, 1),
        ("missing", "test_pkg:mana", True, 1),
    ):
        old = f'.register_attachment_type("{id}", {str(sync).lower()})'
        pattern = rf"\b{caller}\s*{re.escape(old)}"
        matches = list(re.finditer(pattern, changes.source(fixture)))
        if len(matches) != count:
            raise ValueError(f"{fixture}: unexpected {caller} calls for {old}: {matches}")
        expression = f'crate::AttachmentDescriptor::new("{id}", || 37u32)'
        if sync:
            expression += ".serialize(MANA_CODEC).sync()"
        source = changes.source(fixture)
        prefixes = [source[source.rfind('\n', 0, m.start()):m.start()] for m in matches]
        calls = [(prefix + m[0], prefix) for prefix, m in zip(prefixes, matches)]
        for old_call, prefix in set(calls):
            changes.replace(
                fixture, old_call,
                prefix + f"(&mut {caller} as &mut dyn crate::Registrar).register_attachment_type({expression})",
                count=calls.count((old_call, prefix)),
            )


def receivers(changes):
    for path, holder in (
        ("block/part_state.rs", "BlockEntityRef"),
        ("entity/part_state.rs", "EntityRef"),
    ):
        path = VOCAB + path
        changes.replace(path, "const DATA: &'static AttachmentType<Self::Data>;", "fn state_key(&self) -> &AttachmentType<Self::Data>;")
        changes.replace(path, f"fn state(host: &dyn Host, entity: {holder})", f"fn state(&self, host: &dyn Host, entity: {holder})")
        changes.replace(path, "    fn state_existing(\n        host:", "    fn state_existing(\n        &self,\n        host:")
        changes.replace(path, "    fn edit_state<R>(\n        host:", "    fn edit_state<R>(\n        &self,\n        host:")
        changes.replace(path, "Self::state_existing(host, entity)?.unwrap_or_else(Self::DATA.default)", "self.state_existing(host, entity)?.unwrap_or_else(|| (self.state_key().default)())")
        changes.replace(path, "read_state(host, entity, Self::DATA)", "read_state(host, entity, self.state_key())")
        changes.replace(path, "edit_part_state(host, entity, Self::DATA, edit)", "edit_part_state(host, entity, self.state_key(), edit)")
        if holder == "BlockEntityRef":
            for name in ("state_in", "state_existing_in"):
                changes.replace(path, f"    fn {name}(\n        level:", f"    fn {name}(\n        &self,\n        level:")
            changes.replace(path, "Self::state_existing_in(level, entity)?.unwrap_or_else(Self::DATA.default)", "self.state_existing_in(level, entity)?.unwrap_or_else(|| (self.state_key().default)())")
            changes.replace(
                path,
                "        level\n            .block_entity_part_snapshot(entity, Self::DATA.namespace, Self::DATA.name)?",
                "        let key = self.state_key();\n        level\n            .block_entity_part_snapshot(entity, key.namespace, key.name)?",
            )
            changes.replace(path, "Self::DATA", "key", count=3)
            changes.replace(path, "key: &'static AttachmentType<T>,", "key: &AttachmentType<T>,", count=2)

    for path in ("parts/block_entities.rs", "parts/entities.rs"):
        changes.replace(VOCAB + path, "crate::PartStateDeclaration::of(P::DATA)", "crate::PartStateDeclaration::of(part.state_key())")

    press = GAME + "modding/block_entity_part_state_tests.rs"
    charge = GAME + "modding/entity_part_state_tests.rs"
    counter = GAME + "modding/block_entity_part_state_tests/fixture.rs"
    blocks = API_PARTS + "block_entities.rs"
    namespaces = API_PARTS + "owned_namespaces.rs"
    for path, declaration, ty, indent in (
        (press, "struct Press", "i32", ""),
        (charge, "struct Charge", "i32", ""),
        (counter, "pub struct Counter", "i32", ""),
        (blocks, "struct Keeps<const OWNERS: bool>", "u32", "    "),
        (namespaces, "struct Session", "u32", "    "),
    ):
        changes.replace(
            path, f"{indent}{declaration};",
            f"{indent}{declaration} {{\n{indent}    state_key: AttachmentType<{ty}>,\n"
            f"{indent}}}",
        )

    for path, key, ty, indent, visibility in (
        (press, "PROGRESS", "i32", "    ", ""),
        (charge, "CHARGE", "i32", "    ", ""),
        (counter, "COUNT", "i32", "    ", "pub "),
        (blocks, "OWNERS_PROGRESS", "u32", "        ", ""),
        (blocks, "B_PROGRESS", "u32", "        ", ""),
        (namespaces, "SESSION", "u32", "        ", ""),
    ):
        changes.replace(path, f"{visibility}static {key}: AttachmentType<{ty}>", f"{visibility}const {key}: AttachmentType<{ty}>")
        changes.replace(
            path,
            f"{indent}const DATA: &'static AttachmentType<{ty}> = &{key};",
            f"{indent}fn state_key(&self) -> &AttachmentType<{ty}> {{\n"
            f"{indent}    &self.state_key\n{indent}}}",
        )

    changes.replace(press, "Press::edit_state(host, entity, |_, ticks| *ticks += 1)", "self.edit_state(host, entity, |_, ticks| *ticks += 1)")
    changes.replace(counter, "Counter::edit_state(", "self.edit_state(", count=4)
    for path, old, new in (
        (press, "(&kind, Press)", "(&kind, Press { state_key: PROGRESS })"),
        (charge, "(kind, Charge)", "(kind, Charge { state_key: CHARGE })"),
        (counter, "(&kind, Counter)", "(&kind, Counter { state_key: COUNT })"),
        (blocks, "            let _ = r.block_entity_part_with_state(&press, Keeps::<OWNERS>);",
         "            let state_key = if OWNERS { OWNERS_PROGRESS } else { B_PROGRESS };\n"
         "            let _ = r.block_entity_part_with_state(&press, Keeps::<OWNERS> { state_key });"),
        (namespaces, "(&kind, Session)", "(&kind, Session { state_key: SESSION })"),
    ):
        changes.replace(path, old, new)

    # Give the test bodies a Part configured with the same declaration as registration.
    for path, part, holder, key, cases, stored, methods in (
        (press, "Press", "BlockEntityRef", "PROGRESS", 7, 12,
         {"state": 16, "state_existing": 1, "state_in": 4, "state_existing_in": 4, "edit_state": 18}),
        (charge, "Charge", "EntityRef", "CHARGE", 5, 22,
         {"state": 21, "state_existing": 1, "edit_state": 28}),
    ):
        changes.replace(
            path,
            "fn with_state_mod(name: &str, test: impl FnOnce(&mut Game<'_>))",
            f"fn with_state_mod(name: &str, test: impl FnOnce(&mut Game<'_>, &{part}))",
        )
        changes.replace(path, "test(game);", f"test(game, &{part} {{ state_key: {key} }});")
        changes.replace(path, "with_state_mod(NAME, |game|", "with_state_mod(NAME, |game, part|", count=cases)
        changes.replace(path, f"fn stored(game: &mut Game<'_>, entity: {holder})", f"fn stored(part: &{part}, game: &mut Game<'_>, entity: {holder})")
        changes.replace(path, "stored(game, ", "stored(part, game, ", count=stored)
        for name, count in methods.items():
            changes.replace(path, f"{part}::{name}(", f"part.{name}(", count=count)

    for test in ("item_slot_writes_save_but_menu_data_is_transient", "a_nested_item_write_marks_the_level_written_not_the_level_committing"):
        changes.replace(press, f'{test}";\n    with_state_mod(NAME, |game, part|', f'{test}";\n    with_state_mod(NAME, |game, _|')


def main():
    passes = {"imports": imports, "registrations": registrations, "receivers": receivers}
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", choices=passes)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    try:
        changes = Changes(set(WAVE3.read_text(encoding="utf-8").splitlines()))
        passes[args.stage](changes)
    except (OSError, ValueError) as err:
        parser.exit(1, f"{args.stage}: {err}\n")

    # A mismatch or protected path anywhere in the pass refuses it before any writes.
    for path, source in changes.pending.items():
        if args.dry_run:
            print("".join(difflib.unified_diff(
                changes.original[path].splitlines(keepends=True), source.splitlines(keepends=True),
                fromfile=f"a/{path}", tofile=f"b/{path}",
            )), end="")
        else:
            (ROOT / path).write_text(source, encoding="utf-8")


if __name__ == "__main__":
    main()
