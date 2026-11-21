use super::*;
use crate::testing::TestReader;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use sunrise_core::nbt::{Limits, NamedTag, write_named};
use sunrise_mod_runtime::attachments::ATTACHMENTS_KEY;

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
fn codec_error_or_unwind_after_setting_8_refuses_partial_publication() {
    with_listener(1, |calls, value| {
        let job = ChunkDiskJob::capture(&context(ChunkDiskPhase::Save), &store(&value, 7)).unwrap();
        let refusal = run(&job).unwrap_err();
        assert_eq!(refusal.mod_id, "disk_fixture");
        assert_eq!(refusal.reason, "native listener unwound");
        assert_eq!(read(&value, job.captured()), 7);
        assert_eq!(run(&job).unwrap_err(), refusal);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    });
    let initial = install_value();
    let saved = store(&initial, 7).to_nbt();
    let broken = crate::parts::install_attachment_fixture(
        "disk_fixture",
        AttachmentDescriptor::new("disk_fixture:value", || 0i32).serialize(AttachmentCodec {
            encode: |_| Err(AttachmentError::Encode),
            decode: |_| Ok(7),
        }),
    );
    let mut data = ChunkDiskData::from_nbt(saved).unwrap();
    assert!(data.set(&broken, 8).is_err());
    assert!(data.finish().is_err());
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
fn mixed_list_value_budget_includes_the_name_and_both_wrappers() {
    let mixed = |len| Tag::List(vec![Tag::Byte(1), Tag::ByteArray(vec![0; len])]);
    // Eight bytes for the named tag, five for the list header, eight for
    // the two wrappers, one for the byte and four for the array length.
    let value = mixed(65_510);
    let disk = write_named(
        &NamedTag {
            name: "value".into(),
            tag: value.clone(),
        },
        Limits::saved(),
    )
    .unwrap();
    assert_eq!(disk.len(), 65_536);
    assert!(ChunkDiskData::from_nbt(opaque(value)).is_ok());
    let refusal = ChunkDiskData::from_nbt(opaque(mixed(65_511))).unwrap_err();
    assert_eq!(refusal.key, "unknown:value");
    assert_eq!(refusal.reason, "encoded attachment exceeds 65536 bytes");
}

#[test]
fn value_depth_budget_includes_the_mixed_list_wrapper() {
    let mut value = Tag::List(vec![Tag::Byte(1), Tag::Int(2)]);
    for _ in 0..62 {
        value = Tag::List(vec![value]);
    }
    assert!(ChunkDiskData::from_nbt(opaque(value.clone())).is_ok());
    let refusal = ChunkDiskData::from_nbt(opaque(Tag::List(vec![value]))).unwrap_err();
    assert_eq!(refusal.reason, "attachment depth exceeds 64");
}

#[test]
fn chunk_budget_counts_framing_and_modified_utf8_for_empty_namespaces() {
    let empty = || Tag::Compound(Compound::new());
    let mut entries: Compound = ('a'..='o')
        .map(|c| (c.to_string().repeat(65_535).as_str().into(), empty()))
        .collect();
    // The 23-byte attachment frame and 15 namespaces at 65,539 bytes each
    // leave 65,468 bytes. The remaining namespace, envelope and value cost
    // 5 + 5 + 12 bytes before the array, and the last empty namespace costs 20.
    entries.insert(
        "x".into(),
        Tag::compound([(
            "v",
            Tag::compound([("value", Tag::ByteArray(vec![0; 65_426]))]),
        )]),
    );
    let mut last = "\0".repeat(8);
    entries.insert(last.as_str().into(), empty());
    let disk = write_named(
        &NamedTag {
            name: ATTACHMENTS_KEY.into(),
            tag: Tag::Compound(entries.clone()),
        },
        Limits::saved(),
    )
    .unwrap();
    assert_eq!(disk.len(), 1_048_576);
    assert_eq!(
        ChunkDiskData::from_nbt(entries.clone())
            .unwrap()
            .finish()
            .unwrap(),
        entries
    );
    entries.remove(&last.as_str().into());
    last.push('a');
    entries.insert(last.as_str().into(), empty());
    let refusal = ChunkDiskData::from_nbt(entries).unwrap_err();
    assert_eq!(refusal.reason, "chunk attachments exceed 1048576 bytes");
}

#[test]
fn codec_output_that_would_truncate_on_disk_refuses_the_job() {
    let initial = install_value();
    let saved = store(&initial, 7).to_nbt();
    let long_string = crate::parts::install_attachment_fixture(
        "disk_fixture",
        AttachmentDescriptor::new("disk_fixture:value", || 0i32).serialize(AttachmentCodec {
            encode: |_| {
                Ok(Compound::from_iter([(
                    "text".into(),
                    Tag::String("界".repeat(21_846).as_str().into()),
                )]))
            },
            decode: |_| Ok(7),
        }),
    );
    let mut data = ChunkDiskData::from_nbt(saved).unwrap();
    let refusal = data.set(&long_string, 8).unwrap_err();
    assert_eq!(refusal.key, "disk_fixture:value");
    assert_eq!(refusal.reason, "modified UTF-8 exceeds 65535 bytes");
    assert_eq!(data.finish().unwrap_err(), refusal);
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
fn type_without_persistence_codec_refuses_naming_type_and_capability() {
    let initial = install_value();
    let saved = store(&initial, 7).to_nbt();
    let unsaved = crate::parts::install_attachment_fixture(
        "disk_fixture",
        AttachmentDescriptor::new("disk_fixture:value", || 0i32),
    );
    let mut data = ChunkDiskData::from_nbt(saved).unwrap();
    let refusal = data.get(&unsaved).unwrap_err();
    assert_eq!(refusal.key, "disk_fixture:value");
    assert_eq!(refusal.reason, "missing persistence codec");
    // The refusal poisons the whole job even if the listener ignores it.
    assert_eq!(data.finish().unwrap_err(), refusal);
}

#[test]
fn stale_key_refuses_and_poisons_chunk_disk_data() {
    let first = install_value();
    let saved = store(&first, 7).to_nbt();
    let _second = install_value();

    let mut data = ChunkDiskData::from_nbt(saved.clone()).unwrap();
    let refusal = data.get(&first).unwrap_err();
    assert_eq!(refusal.key, "disk_fixture:value");
    assert_eq!(refusal.reason, "StaleKey");
    assert_eq!(data.finish().unwrap_err(), refusal);

    let mut data = ChunkDiskData::from_nbt(saved.clone()).unwrap();
    assert_eq!(data.set(&first, 8).unwrap_err(), refusal);

    let mut data = ChunkDiskData::from_nbt(saved).unwrap();
    assert_eq!(data.remove(&first).unwrap_err(), refusal);
}
