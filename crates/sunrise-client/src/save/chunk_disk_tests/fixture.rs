//! The attachment and its save/load edits used by the region writer tests.
use mod_api::{
    AttachmentCodec, AttachmentError, AttachmentKey, ChunkDiskData, ChunkDiskHost, Compound, Tag,
};

use sunrise_mod_runtime::attachments::AttachmentType;

pub const VALUE: AttachmentType<i32> = AttachmentType {
    namespace: "chunk_disk_fixture",
    name: "disk_value",
    default: || 0,
    codec: Some(CODEC),
    copy_on_death: false,
    sync: false,
};

const CODEC: AttachmentCodec<i32> = AttachmentCodec {
    encode: |value| Ok(Compound::from_iter([("counter".into(), Tag::Int(*value))])),
    decode: |tag| match tag.get(&"counter".into()) {
        Some(Tag::Int(value)) => Ok(*value),
        _ => Err(AttachmentError::Decode),
    },
};

pub fn register(registrar: &mut dyn mod_api::Registrar) -> AttachmentKey<i32> {
    registrar
        .register_attachment_type(
            mod_api::AttachmentDescriptor::new("chunk_disk_fixture:disk_value", VALUE.default)
                .serialize(CODEC),
        )
        .unwrap()
}

/// Only the chunk that the test fills with a value needs editing.
pub fn save(data: &mut ChunkDiskData, key: &AttachmentKey<i32>) {
    let value = data.get(key).unwrap();
    if value == 0 {
        return;
    }
    assert_eq!(value, 7);
    data.set(key, 8).unwrap();
}

pub fn load(_: &dyn ChunkDiskHost, data: &mut ChunkDiskData, key: &AttachmentKey<i32>) {
    let value = data.get(key).unwrap();
    if value != 0 {
        data.set(key, value + 1).unwrap();
    }
}
