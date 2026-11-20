//! A block entity Part whose tick exercises the Part state accessors.
use mod_api::*;

const COUNT_CODEC: mod_api::AttachmentCodec<i32> = AttachmentCodec {
    encode: |n| Ok([("n".into(), Tag::Int(*n))].into()),
    decode: |saved| match saved.get(&"n".into()) {
        Some(Tag::Int(n)) => Ok(*n),
        _ => Err(AttachmentError::Decode),
    },
};

pub const COUNT: AttachmentType<i32> = AttachmentType {
    namespace: "part_state_fixture",
    name: "part_state_count",
    default: || 0,
    codec: Some(COUNT_CODEC),
    copy_on_death: false,
    sync: false,
};

const MARK_CODEC: mod_api::AttachmentCodec<i32> = AttachmentCodec {
    encode: |n| Ok([("n".into(), Tag::Int(*n))].into()),
    decode: |saved| match saved.get(&"n".into()) {
        Some(Tag::Int(n)) => Ok(*n),
        _ => Err(AttachmentError::Decode),
    },
};

/// Set to 1 after the undecodable-value check, so the game sees it ran to the end.
pub static MARK: AttachmentType<i32> = AttachmentType {
    namespace: "part_state_fixture",
    name: "part_state_mark",
    default: || 0,
    codec: Some(MARK_CODEC),
    copy_on_death: false,
    sync: false,
};

/// Each tick does what its block entity's first data slot says:
/// - 0: an aborted edit, a refused recursive edit, then a committed count;
/// - 1: an edit of an undecodable saved value is refused before its closure runs, then `MARK`
///   is set to 1.
pub struct Counter {
    state_key: AttachmentType<i32>,
}
#[mod_api::sunrise_part]
impl BlockPart for Counter {}
#[mod_api::sunrise_part]
impl BlockEntityPart for Counter {
    fn block_entity_tick(&self, host: &mut dyn ServerHost, event: &mut BlockEntityTickEvent<'_>) {
        let level = host.level(host.dimension()).unwrap();
        let entity = host.block_entity_identity(level, event.pos).unwrap();
        match event.entity.data[0] {
            1 => {
                let refused = self.edit_state(host, entity, |_, _| panic!("ran"));
                assert_eq!(
                    refused,
                    Err(PartStateError::Attachment {
                        namespace: "part_state_fixture",
                        name: "part_state_count",
                        error: AttachmentError::Decode,
                    })
                );
                assert_eq!(edit_part_state(host, entity, &MARK, |_, m| *m = 1), Ok(()));
            }
            _ => {
                let aborted: Result<(), ()> = host.transaction(|host, _| {
                    assert_eq!(self.edit_state(host, entity, |_, n| *n = 100), Ok(()));
                    Err(())
                });
                assert_eq!(aborted, Err(()));
                let recursive = self.edit_state(host, entity, |host, n| {
                    *n += 1;
                    self.edit_state(host, entity, |_, n| *n = 50)
                });
                assert_eq!(recursive, Ok(Err(PartStateError::Reentrant)));
            }
        }
    }
}
impl BlockEntityPartState for Counter {
    type Data = i32;
    fn state_key(&self) -> &AttachmentType<i32> {
        &self.state_key
    }
}

pub fn register(registrar: &mut dyn Registrar) {
    registrar
        .register_attachment_type(
            mod_api::AttachmentDescriptor::new(
                "part_state_fixture:part_state_count",
                COUNT.default,
            )
            .serialize(COUNT_CODEC),
        )
        .unwrap();
    registrar
        .register_attachment_type(
            mod_api::AttachmentDescriptor::new("part_state_fixture:part_state_mark", MARK.default)
                .serialize(MARK_CODEC),
        )
        .unwrap();
    let kind = registrar
        .block_entity_type("part_state")
        .blocks(&["part_state_fixture:part_state"])
        .build();
    registrar
        .block_entity_part_with_state(&kind, Counter { state_key: COUNT })
        .unwrap();
}

/// The block the state's block entity stands in, so a save keeps it.
pub fn blocks(event: &mut BlocksEvent<'_>) {
    event
        .register(Block::new("part_state_fixture:part_state", "minecraft:barrel").with_item(false))
        .unwrap();
}
