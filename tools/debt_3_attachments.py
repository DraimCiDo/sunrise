#!/usr/bin/env python3
"""Replay the mechanical steps for typed attachment registration (debt item 3).

Run `imports` first. `registrations` and `receivers` are for the commits that
introduce AttachmentDescriptor/AttachmentKey and make Part state instance-based.
Those passes leave key ownership, mixed raw/host fixtures and negative cases for
hand fixes. Use --dry-run to inspect a pass without writing files.
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


def descriptor(changes, path, head, qualified, attachment_id, sync):
    """Change only the literal's type; keep its entire Rust initializer verbatim."""
    pattern = rf"^(?P<indent> *){re.escape(head)}\n(?P<body>.*?)^(?P=indent)\}};"
    matches = list(re.finditer(pattern, changes.source(path), re.MULTILINE | re.DOTALL))
    if len(matches) != 1:
        raise ValueError(f"{path}: expected exactly one descriptor starting with {head!r}")
    match = matches[0]
    prefix = match["indent"] + "    "
    namespace, name = attachment_id.split(":", 1)
    fields = match["body"].splitlines()
    for field in (f'namespace: "{namespace}",', f'name: "{name}",', f"sync: {sync},"):
        if fields.count(prefix + field) != 1:
            raise ValueError(f"{path}: {head!r} does not declare {field!r}")
    new_head = re.sub(r"(?:mod_api::)?AttachmentType\b", f"{qualified}::AttachmentDescriptor", head)
    changes.replace(
        path, match[0], match["indent"] + new_head + "\n" + match["body"] + match["indent"] + "};"
    )


def register_call(
    changes, path, attachment_id, sync, symbol, receiver, *,
    chain=None, indent=None, statement=8, count=1,
):
    """Pair a whole literal name/sync call with a descriptor checked above."""
    args = f'.register_attachment_type("{attachment_id}", {sync})'
    old = f"{receiver}{args}.unwrap();"
    if indent is not None:
        statement = indent - 4
        old = f"{receiver}{args}\n{' ' * indent}.unwrap();"
    if chain is not None:
        statement = chain - 4
        old = f"{receiver}\n{' ' * chain}{args}\n{' ' * chain}.unwrap();"
    new = f"let {symbol.lower()}_key = {symbol}.register({receiver}).unwrap();"
    start = "\n" + " " * statement
    changes.replace(path, start + old + "\n", start + new + "\n", count=count)


def registrations(changes):
    # Only match known declarations. Tests that build a descriptor with struct
    # update syntax or intentionally register a wrong/duplicate name are hand fixes.
    # The stored key, not the const descriptor, must be used by mod callbacks.
    for path, declaration, attachment_id, sync, receiver, options in (
        ("entities/general/tick_tests.rs", "static COUNTER", "mymod:counter", "false", "r", {}),
        (
            "entities/passengers.rs", "static VEHICLE_VALUE", "vehicle_boundary:value", "false",
            "registrar", {"chain": 16, "count": 2},
        ),
        (
            "mobs/creatures/borrowed_panic_tests.rs", "const VALUE", "borrowed-panic:retained",
            "false", "r", {"indent": 12},
        ),
        (
            "modding/attachment_sync/transport_tests.rs", "pub(super) static VALUE",
            "sync_route:value", "true", "registrar", {"chain": 12},
        ),
        (
            "modding/attachment_sync/transport_tests.rs", "static BLOB", "sync_route:blob",
            "true", "registrar", {"chain": 12},
        ),
        (
            "modding/block_entity_part_state_tests.rs", "static PROGRESS", "part_state:progress",
            "false", "r", {"indent": 12},
        ),
        (
            "modding/entity_initialization/tests.rs", "const SAVED", "constructing:saved",
            "false", "r", {"indent": 12},
        ),
        (
            "modding/entity_part_state_tests.rs", "static CHARGE", "part_state:charge",
            "false", "r", {"indent": 12},
        ),
        ("modding/native_general_renderers_tests.rs", "static FUEL", "test:fuel", "true", "r", {}),
        (
            "modding/native_general_renderers_tests/fixture.rs", "pub static FUEL",
            "general_render_fixture:render_fuel", "true", "registrar", {"chain": 8},
        ),
        (
            "native_hooks/shared/item_fall_on_tests.rs", "static LOAN_VALUE", "loan_test:progress",
            "false", "r", {"indent": 12},
        ),
        (
            "runtime/transaction_entities_tests.rs", "static MARK", "journal:mark", "false", "r", {},
        ),
        (
            "world/server_travel.rs", "const STATE", "natural_tree:state", "false", "r",
            {"indent": 20},
        ),
    ):
        path = GAME + path
        symbol = declaration.rsplit(" ", 1)[1]
        raw_type = (
            "mod_api::AttachmentType" if symbol in {"VEHICLE_VALUE", "LOAN_VALUE"}
            else "AttachmentType"
        )
        value_type = "Vec<u8>" if symbol == "BLOB" else "i32"
        head = f"{declaration}: {raw_type}<{value_type}> = {raw_type} {{"
        descriptor(changes, path, head, "mod_api", attachment_id, sync)
        register_call(changes, path, attachment_id, sync, symbol, receiver, **options)

    constructing = GAME + "modding/entity_initialization/tests.rs"
    descriptor(changes, constructing, "const VALUE: AttachmentType<i32> = AttachmentType {", "mod_api", "constructing:value", "false")
    register_call(changes, constructing, "constructing:value", "false", "VALUE", "r", indent=12, count=2)
    register_call(changes, constructing, "constructing:value", "false", "VALUE", "r", indent=16)

    # The snapshot test already imports VALUE from the transport fixture above.
    snapshot = GAME + "modding/attachment_sync/general_snapshot_tests.rs"
    changes.expect(snapshot, "use super::transport_tests::{VALUE, store};")
    register_call(changes, snapshot, "sync_route:value", "true", "VALUE", "r", indent=12)

    fixture = GAME + "modding/block_entity_part_state_tests/fixture.rs"
    for symbol, name in (("COUNT", "part_state_count"), ("MARK", "part_state_mark")):
        descriptor(changes, fixture, f"pub static {symbol}: AttachmentType<i32> = AttachmentType {{", "mod_api", f"part_state_fixture:{name}", "false")
    changes.replace(
        fixture,
        '    for name in ["part_state_count", "part_state_mark"] {\n'
        '        registrar\n'
        '            .register_attachment_type(&format!("part_state_fixture:{name}"), false)\n'
        '            .unwrap();\n'
        '    }',
        '    let count_key = COUNT.register(registrar).unwrap();\n'
        '    let mark_key = MARK.register(registrar).unwrap();',
    )

    for path, symbol, attachment_id, indent in (
        ("block_entities.rs", "OWNERS_PROGRESS", "owner:progress", 16),
        ("block_entities.rs", "B_PROGRESS", "b:progress", 12),
        ("owned_namespaces.rs", "SESSION", "porter:session", 12),
    ):
        path = API_PARTS + path
        descriptor(
            changes, path, f"static {symbol}: AttachmentType<u32> = AttachmentType {{",
            "crate", attachment_id, "false",
        )
        register_call(changes, path, attachment_id, "false", symbol, "r", statement=indent)


def receivers(changes):
    # Switch trait implementations to a field on their Part. The registration
    # wiring constructs those Parts using the owned keys in a subsequent edit.
    for path, holder, static_refs in (
        ("block/part_state.rs", "BlockEntityRef", 9),
        ("entity/part_state.rs", "EntityRef", 3),
    ):
        path = VOCAB + path
        changes.replace(path, "const DATA: &'static AttachmentType<Self::Data>;", "fn state_key(&self) -> &AttachmentKey<Self::Data>;")
        changes.replace(path, f"fn state(host: &dyn Host, entity: {holder})", f"fn state(&self, host: &dyn Host, entity: {holder})")
        changes.replace(path, "    fn state_existing(\n        host:", "    fn state_existing(\n        &self,\n        host:")
        changes.replace(path, "    fn edit_state<R>(\n        host:", "    fn edit_state<R>(\n        &self,\n        host:")
        changes.replace(path, "Self::state_existing(host, entity)", "self.state_existing(host, entity)")
        if holder == "BlockEntityRef":
            for name in ("state_in", "state_existing_in"):
                changes.replace(path, f"    fn {name}(\n        level:", f"    fn {name}(\n        &self,\n        level:")
            changes.replace(path, "Self::state_existing_in(level, entity)", "self.state_existing_in(level, entity)")
        changes.replace(path, "Self::DATA", "self.state_key()", count=static_refs)

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
            f"{indent}{declaration} {{\n{indent}    state_key: mod_api::AttachmentKey<{ty}>,\n"
            f"{indent}}}",
        )

    for path, key, ty, indent in (
        (press, "PROGRESS", "i32", "    "),
        (charge, "CHARGE", "i32", "    "),
        (counter, "COUNT", "i32", "    "),
        (blocks, "OWNERS_PROGRESS", "u32", "        "),
        (blocks, "B_PROGRESS", "u32", "        "),
        (namespaces, "SESSION", "u32", "        ")
    ):
        changes.replace(
            path,
            f"{indent}const DATA: &'static AttachmentType<{ty}> = &{key};",
            f"{indent}fn state_key(&self) -> &mod_api::AttachmentKey<{ty}> {{\n"
            f"{indent}    &self.state_key\n{indent}}}",
        )

    changes.replace(press, "Press::edit_state(host, entity, |_, ticks| *ticks += 1)", "self.edit_state(host, entity, |_, ticks| *ticks += 1)")
    changes.replace(counter, "Counter::edit_state(", "self.edit_state(", count=4)
    # The fixture setup supplies one `part` using the test installation's key.
    for path, part, methods in (
        (press, "Press", {"state": 16, "state_existing": 1, "state_in": 4, "state_existing_in": 4, "edit_state": 18}),
        (charge, "Charge", {"state": 21, "state_existing": 1, "edit_state": 28}),
    ):
        for name, count in methods.items():
            changes.replace(path, f"{part}::{name}(", f"part.{name}(", count=count)


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
