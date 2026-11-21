use super::*;
use crate::testing::TestReader;
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use sunrise_core::nbt::DISK_WRITE_NESTING_GUARD;

fn value_codec() -> AttachmentCodec<i32> {
    AttachmentCodec {
        encode: |value| Ok(Compound::from_iter([("value".into(), Tag::Int(*value))])),
        decode: |tag| match tag.get(&"value".into()) {
            Some(Tag::Int(value)) => Ok(*value),
            _ => Err(AttachmentError::Decode),
        },
    }
}

fn install_value() -> AttachmentKey<i32> {
    crate::parts::install_attachment_fixture(
        "disk_fixture",
        AttachmentDescriptor::new("disk_fixture:value", || 0i32).serialize(value_codec()),
    )
}

fn store(key: &AttachmentKey<i32>, value: i32) -> Attachments {
    let mut store = Attachments::default();
    key.live()
        .unwrap()
        .set(&mut store, Holder::Level, value)
        .unwrap();
    store
}
fn context(phase: ChunkDiskPhase) -> ChunkDiskContext {
    ChunkDiskContext::new(
        WorldEpoch::new(17),
        Identifier::parse("minecraft:overworld").unwrap(),
        [2, -3],
        ChunkDiskKind::Full,
        phase,
    )
}
fn read(key: &AttachmentKey<i32>, entries: &Compound) -> i32 {
    key.live()
        .unwrap()
        .get(&mut Attachments::from_nbt(entries.clone()), Holder::Level)
        .unwrap()
}
fn run(job: &ChunkDiskJob) -> Result<Compound, ChunkDiskFailure> {
    let reader = TestReader::default();
    job.run(&OwningWorldgen::new(&reader, &|_, _, _| None))
        .clone()
}
struct Listener {
    calls: Arc<AtomicUsize>,
    mode: u8,
    value: AttachmentKey<i32>,
}
#[crate::__subscriptions]
impl Hooks for Listener {
    fn chunk_data_save(&self, host: &dyn ChunkDiskHost, event: &mut ChunkDataSaveEvent<'_>) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert!(host.backend().is_none());
        assert_eq!(event.data.get(&self.value).unwrap(), 7);
        event.data.set(&self.value, 8).unwrap();
        if self.mode == 1 {
            panic!("fixture unwind after tentative write");
        }
    }
    fn chunk_data_load(&self, _: &dyn ChunkDiskHost, event: &mut ChunkDataLoadEvent<'_>) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let value = event.data.get(&self.value).unwrap();
        event.data.set(&self.value, value + 1).unwrap();
    }
}
fn with_listener(mode: u8, apply: impl FnOnce(Arc<AtomicUsize>, AttachmentKey<i32>)) {
    let calls = Arc::new(AtomicUsize::new(0));
    let value = install_value();
    testing::with_dispatch(
        vec![ModEntry::new(
            "disk_fixture",
            Box::new(Listener {
                calls: calls.clone(),
                mode,
                value: value.clone(),
            }),
        )]
        .into(),
        || apply(calls, value),
    );
}

#[test]
fn host_borrows_the_captured_context() {
    let captured = context(ChunkDiskPhase::Save);
    let reader = TestReader::default();
    let resources = OwningWorldgen::new(&reader, &|_, _, _| None);
    let host: &dyn ChunkDiskHost = &OwningChunkDisk::new(&captured, &resources);
    assert!(std::ptr::eq(host.job(), &captured));
}

#[test]
fn capture_7_callback_8_live_9() {
    with_listener(0, |calls, value| {
        let mut live = store(&value, 7);
        let job = ChunkDiskJob::capture(&context(ChunkDiskPhase::Save), &live).unwrap();
        value
            .live()
            .unwrap()
            .set(&mut live, Holder::Level, 9)
            .unwrap();
        assert_eq!(read(&value, &run(&job).unwrap()), 8);
        assert_eq!(
            value.live().unwrap().get(&mut live, Holder::Level).unwrap(),
            9
        );
        assert_eq!(read(&value, &run(&job).unwrap()), 8);
        assert_eq!(read(&value, job.captured()), 7);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    });
}

#[test]
fn load_disk_8_callback_increment_publishes_9_once() {
    with_listener(0, |calls, value| {
        let job = ChunkDiskJob::capture(&context(ChunkDiskPhase::Load), &store(&value, 8)).unwrap();
        assert_eq!(read(&value, &run(&job).unwrap()), 9);
        assert_eq!(read(&value, &run(&job).unwrap()), 9);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    });
}

#[test]
fn unwind_after_setting_8_refuses_partial_publication_and_codec_error_is_local() {
    with_listener(1, |calls, value| {
        let job = ChunkDiskJob::capture(&context(ChunkDiskPhase::Save), &store(&value, 7)).unwrap();
        let refusal = run(&job).unwrap_err();
        assert_eq!(refusal.mod_id, "disk_fixture");
        assert_eq!(refusal.reason, "native listener unwound");
        assert_eq!(read(&value, job.captured()), 7);
        assert_eq!(run(&job).unwrap_err(), refusal);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    });
    let (broken_record, broken) = crate::parts::registered_attachment_fixture(
        "disk_fixture",
        AttachmentDescriptor::new("disk_fixture:broken", || 0i32).serialize(AttachmentCodec {
            encode: |_| Err(AttachmentError::Encode),
            decode: |_| Ok(7),
        }),
    );
    let (good_record, good) = crate::parts::registered_attachment_fixture(
        "disk_fixture",
        AttachmentDescriptor::new("disk_fixture:value", || 0i32).serialize(value_codec()),
    );
    crate::parts::install_input(crate::parts::installation::InstalledInput {
        attachment_types: HashMap::from([(
            "disk_fixture",
            HashMap::from([("broken", broken_record), ("value", good_record)]),
        )]),
        ..Default::default()
    });
    let saved = store(&good, 7).to_nbt();
    let mut data = ChunkDiskData::from_nbt(saved).unwrap();
    assert!(data.set(&broken, 8).is_err());
    // A local codec failure must not poison unrelated edits in the same job.
    data.set(&good, 9).unwrap();
    assert_eq!(read(&good, &data.finish().unwrap()), 9);
}

fn opaque(value: Tag) -> Compound {
    Compound::from_iter([(
        "unknown".into(),
        Tag::Compound(Compound::from_iter([(
            "value".into(),
            Tag::Compound(Compound::from_iter([("value".into(), value)])),
        )])),
    )])
}

#[test]
fn disk_validation_allows_vanilla_sized_values_and_enforces_writer_nesting_guard() {
    // Values above the old ad hoc 65,536-byte and depth-64 caps pass real disk validation.
    let large = opaque(Tag::ByteArray(vec![0; 70_000]));
    assert_eq!(
        ChunkDiskData::from_nbt(large.clone())
            .unwrap()
            .finish()
            .unwrap(),
        large
    );
    let mut deep = Tag::Int(1);
    for _ in 0..70 {
        deep = Tag::List(vec![deep]);
    }
    let deep_entries = opaque(deep);
    assert_eq!(
        ChunkDiskData::from_nbt(deep_entries.clone())
            .unwrap()
            .finish()
            .unwrap(),
        deep_entries
    );

    // Nesting past the real disk writer guard is still rejected.
    let mut too_deep = Tag::Int(1);
    for _ in 0..=DISK_WRITE_NESTING_GUARD {
        too_deep = Tag::List(vec![too_deep]);
    }
    assert!(ChunkDiskData::from_nbt(opaque(too_deep)).is_err());
}

#[test]
fn disk_operations_use_only_persistence_codec_and_never_invoke_sync_codec() {
    let dual = crate::parts::install_attachment_fixture(
        "disk_fixture",
        AttachmentDescriptor::new("disk_fixture:dual", || 5i32)
            .serialize(value_codec())
            .sync(AttachmentCodec {
                encode: |_| Err(AttachmentError::Encode),
                decode: |_| Err(AttachmentError::Decode),
            }),
    );
    let mut data = ChunkDiskData::from_nbt(Compound::new()).unwrap();
    assert_eq!(data.get(&dual).unwrap(), 5);
    data.set(&dual, 12).unwrap();
    assert_eq!(data.get(&dual).unwrap(), 12);
    let saved = data.clone().finish().unwrap();
    let mut reload = ChunkDiskData::from_nbt(saved).unwrap();
    assert_eq!(reload.get(&dual).unwrap(), 12);
    assert_eq!(reload.remove(&dual).unwrap(), Some(12));
    let after_remove = reload.finish().unwrap();
    let mut verify = ChunkDiskData::from_nbt(after_remove).unwrap();
    assert_eq!(verify.remove(&dual).unwrap(), None);
}

#[test]
fn unknown_attachment_survives_typed_edit() {
    let value = install_value();
    let mut data = ChunkDiskData::from_nbt(opaque(Tag::Int(7))).unwrap();
    data.set(&value, 8).unwrap();
    let result = data.finish().unwrap();
    assert_eq!(
        result.get(&"unknown".into()),
        opaque(Tag::Int(7)).get(&"unknown".into())
    );
}

#[test]
fn zero_subscribers_over_100_saves_creates_no_jobs_or_callbacks() {
    let value = install_value();
    testing::with_dispatch(Vec::new().into(), || {
        let snapshot = store(&value, 7);
        let context = context(ChunkDiskPhase::Save);
        for _ in 0..100 {
            assert!(ChunkDiskJob::capture(&context, &snapshot).is_none());
        }
    });
}

#[test]
fn local_transaction_abort_preserves_prior_snapshot() {
    let value = install_value();
    let mut data = ChunkDiskData::from_nbt(store(&value, 7).to_nbt()).unwrap();
    let result: Result<(), ()> = data.transaction(|data| {
        data.set(&value, 8).unwrap();
        Err(())
    });
    assert_eq!(result, Err(()));
    assert_eq!(read(&value, &data.finish().unwrap()), 7);
}

#[test]
fn type_without_persistence_codec_refuses_locally_naming_type_and_capability() {
    let (unsaved_record, unsaved) = crate::parts::registered_attachment_fixture(
        "disk_fixture",
        AttachmentDescriptor::new("disk_fixture:unsaved", || 0i32),
    );
    let (saved_record, value) = crate::parts::registered_attachment_fixture(
        "disk_fixture",
        AttachmentDescriptor::new("disk_fixture:value", || 0i32).serialize(value_codec()),
    );
    crate::parts::install_input(crate::parts::installation::InstalledInput {
        attachment_types: HashMap::from([(
            "disk_fixture",
            HashMap::from([("unsaved", unsaved_record), ("value", saved_record)]),
        )]),
        ..Default::default()
    });
    let saved = store(&value, 7).to_nbt();
    let mut data = ChunkDiskData::from_nbt(saved).unwrap();
    let refusal = data.get(&unsaved).unwrap_err();
    assert_eq!(refusal.key, "disk_fixture:unsaved");
    assert_eq!(refusal.reason, "missing persistence codec");
    // The local refusal does not poison unrelated valid edits.
    data.set(&value, 10).unwrap();
    assert_eq!(read(&value, &data.finish().unwrap()), 10);
}

#[test]
fn stale_key_refuses_locally_without_poisoning_unrelated_edits() {
    let stale = install_value();
    let saved = store(&stale, 7).to_nbt();
    let (fresh_record, fresh) = crate::parts::registered_attachment_fixture(
        "disk_fixture",
        AttachmentDescriptor::new("disk_fixture:value", || 0i32).serialize(value_codec()),
    );
    let (other_record, other) = crate::parts::registered_attachment_fixture(
        "other_mod",
        AttachmentDescriptor::new("other_mod:other", || 0i32).serialize(value_codec()),
    );
    crate::parts::install_input(crate::parts::installation::InstalledInput {
        attachment_types: HashMap::from([
            ("disk_fixture", HashMap::from([("value", fresh_record)])),
            ("other_mod", HashMap::from([("other", other_record)])),
        ]),
        ..Default::default()
    });

    let mut data = ChunkDiskData::from_nbt(saved).unwrap();
    let refusal = data.get(&stale).unwrap_err();
    assert_eq!(refusal.key, "disk_fixture:value");
    assert_eq!(refusal.reason, "StaleKey");
    assert_eq!(data.set(&stale, 8).unwrap_err(), refusal);
    assert_eq!(data.remove(&stale).unwrap_err(), refusal);

    // Unrelated valid edits on live keys still succeed and publish cleanly.
    data.set(&other, 42).unwrap();
    assert_eq!(data.get(&fresh).unwrap(), 7);
    let published = data.finish().unwrap();
    assert_eq!(read(&fresh, &published), 7);
    assert_eq!(read(&other, &published), 42);
}
