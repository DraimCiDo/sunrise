#!/usr/bin/env python3
"""Mechanical half of debt 3 step 6, run on 0316b7026d2 before fixture wiring."""
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "crates"
GAME = "sunrise-client/src/game/"
MODDING = GAME + "modding/"
INDEX = MODDING + "attachment_index/"
API = "sunrise-mod-api/"


def edit(name, transform):
    path = ROOT / name
    source = path.read_text()
    result = transform(source)
    assert result != source, name
    path.write_text(result)


def replace(source, old, new):
    assert old in source, old
    return source.replace(old, new)


def drop_raw(source, name):
    return replace(
        source,
        re.search(rf"(?m)^ *?(?:pub(?:\(crate\))? )?(?:const|static) {name}: .*?\n *?\}};\n\n?", source, re.S)[0],
        "",
    )


for filename in ["query_tests.rs", "mob_tests.rs", "saved_entity_tests.rs"]:
    def migrate(source):
        source = drop_raw(source, "VALUE")
        source = re.sub(r", AttachmentType(?=[},])", "", source)
        source = replace(source, "VALUE.default", "|| 0")
        if filename != "query_tests.rs":
            source = source.replace("let key = (\"", "let name = (\"").replace(".snapshot(&key)", ".snapshot(&name)")
        if filename == "mob_tests.rs":
            source = replace(source, "{with_game, with_game_attachment}", "with_game_attachment")
            source = replace(source, "with_game(|game| {", "with_fixture(|game, key| {")
        return re.sub(r"\bVALUE(?=\s*\.)", "key", source)

    edit(INDEX + filename, migrate)

edit(INDEX + "attached_storage_tests.rs", lambda s: drop_raw(s, "ITEMS")
     .replace(" AttachmentType,", "").replace(" StackList,", "")
     .replace("ITEMS.default", "Vec::new").replace("AttachedItems::new(items,", "AttachedItems::new(items.clone(),")
     .replace("ITEMS.has(", "items.has(").replace("ITEMS.get_existing(", "items.get_existing(")
     .replace("let codec = ITEMS.codec.as_ref().unwrap();", "let codec = ITEM_STACKS;"))

edit(MODDING + "native_general_renderers_tests/fixture.rs", lambda s: drop_raw(s, "FUEL")
     .replace(" AttachmentType,", "").replace("FUEL.default", "|| 0"))

edit(GAME + "network/remote/attachment_query_tests.rs", lambda s: s
     .replace("AttachmentCodec, AttachmentType, Host", "AttachmentCodec, Host")
     .replace("let host: &mut dyn Host = game;\n", "")
     .replace("VALUE.set(host.attachments(),", "key.set(game,").replace("VALUE.get(host.attachments(),", "key.get(game,")
     .replace("const VALUE: AttachmentType<i32> = AttachmentType {\n    namespace: \"mirror\",\n    name: \"value\",\n    default: || 73,\n    codec: Some(AttachmentCodec {", "const VALUE_CODEC: AttachmentCodec<i32> = AttachmentCodec {")
     .replace("    }),\n    copy_on_death: false,\n    sync: true,\n", "")
     .replace("VALUE.default", "|| 73").replace("VALUE.codec.unwrap()", "VALUE_CODEC"))

edit(API + "src/parts/capabilities.rs", lambda s: drop_raw(drop_raw(s, "ROBOT_ENERGY"), "ROBOT_FLUID")
     .replace("    /// A fluid attachment type for the tests.\n", ""))
edit(API + "src/parts/block_entities.rs", lambda s: drop_raw(s, "HOARD")
     .replace("its `HOARD` counts", "its attachment counts")
     .replace("struct Hoard(pub(crate) BlockKey);", "struct Hoard(pub(crate) BlockKey, pub(crate) AttachmentKey<u32>);")
     .replace("HOARD.get_existing(", "self.1.get_existing("))
edit(API + "src/parts/entities.rs", lambda s: s
     .replace("struct EntityBattery(pub(crate) EntityRef);", "struct EntityBattery(pub(crate) EntityRef, pub(crate) AttachmentKey<u32>);")
     .replace("struct EntityTank(pub(crate) EntityRef);", "struct EntityTank(pub(crate) EntityRef, pub(crate) AttachmentKey<u32>);")
     .replace("            ROBOT_ENERGY\n", "            self.1\n").replace("            ROBOT_FLUID\n", "            self.1\n"))

# The following descriptors are read exclusively by engine fixtures on raw maps or
# index entries; switch their imports rather than exposing them on the mod API.
edit(GAME + "entities/player_residency_tests.rs", lambda s: replace(
    s, "mod_api::AttachmentType", "sunrise_mod_runtime::attachments::AttachmentType"))
edit(GAME + "entities/passengers.rs", lambda s: replace(
    s, "mod_api::AttachmentType", "sunrise_mod_runtime::attachments::AttachmentType"))
edit("sunrise-client/src/save/chunk_disk_tests/fixture.rs", lambda s: s
     .replace(" AttachmentType,", "").replace("\npub const VALUE:", "\nuse sunrise_mod_runtime::attachments::AttachmentType;\n\npub const VALUE:"))
