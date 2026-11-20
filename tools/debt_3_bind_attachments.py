#!/usr/bin/env python3
"""Step 5 of attachment debt: mechanical type/import migration at consumer boundaries."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "crates"

VOCAB = [
    "attachment_queries.rs",
    "block/part_state.rs",
    "chunk_disk/data.rs",
    "client/entity_render_attachments.rs",
    "content/attached_storage.rs",
    "entity/attachment_query.rs",
    "entity/part_state.rs",
    "entity/types.rs",
]

for name in VOCAB:
    path = ROOT / "sunrise-mod-vocab/src" / name
    text = path.read_text()
    # Leave raw test descriptors for the fixture migration.
    source, separator, tests = text.partition("#[cfg(test)]\nmod tests {")
    assert "AttachmentType" in source, name
    source = source.replace("AttachmentType", "AttachmentKey")
    if name == "content/attached_storage.rs":
        source = source.replace("&'static AttachmentKey", "AttachmentKey")
    path.write_text(source + separator + tests)

for name in [
    "sunrise-client/src/game/modding/block_entity_part_state_tests.rs",
    "sunrise-client/src/game/modding/block_entity_part_state_tests/fixture.rs",
    "sunrise-client/src/game/modding/entity_part_state_tests.rs",
    "sunrise-mod-api/src/parts/block_entities.rs",
    "sunrise-mod-api/src/parts/owned_namespaces.rs",
]:
    path = ROOT / name
    text = path.read_text()
    assert "state_key: AttachmentType<" in text and "-> &AttachmentType<" in text, name
    path.write_text(text.replace("state_key: AttachmentType<", "state_key: crate::AttachmentKey<")
                         .replace("-> &AttachmentType<", "-> &crate::AttachmentKey<")
                         .replace("crate::AttachmentKey", "mod_api::AttachmentKey"))
