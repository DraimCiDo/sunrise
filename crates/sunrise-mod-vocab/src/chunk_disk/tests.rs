use super::*;
use crate::testing::TestReader;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use sunrise_core::nbt::{Limits, NamedTag, read_named, write_named};
use sunrise_mod_runtime::attachments::ATTACHMENTS_KEY;

const VALUE: AttachmentType<i32> = AttachmentType {
    namespace: "disk_fixture",
    name: "value",
    default: || 0,
    codec: Some(sunrise_mod_runtime::attachments::AttachmentCodec {
        encode: |value| Ok(Compound::from_iter([("value".into(), Tag::Int(*value))])),
        decode: |tag| match tag.get(&"value".into()) {
            Some(Tag::Int(value)) => Ok(*value),
            _ => Err(AttachmentError::Decode),
        },
    }),
    copy_on_death: false,
    sync: false,
};

fn store(value: i32) -> Attachments {
    let mut store = Attachments::default();
    VALUE.set(&mut store, Holder::Level, value).unwrap();
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
fn read(entries: &Compound) -> i32 {
    VALUE
        .get(&mut Attachments::from_nbt(entries.clone()), Holder::Level)
        .unwrap()
}
fn run(job: ChunkDiskJob) -> Compound {
    let reader = TestReader::default();
    job.run(&OwningWorldgen::new(&reader, &|_, _, _| None))
}
struct Listener {
    calls: Arc<AtomicUsize>,
    panic_after_edit: bool,
}
#[crate::__subscriptions]
impl Hooks for Listener {
    fn chunk_data_save(&self, host: &dyn ChunkDiskHost, event: &mut ChunkDataSaveEvent<'_>) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert!(host.backend().is_none());
        let value = event.data.get(&VALUE).unwrap();
        event.data.set(&VALUE, value + 1).unwrap();
        if self.panic_after_edit {
            panic!("fixture unwind after attachment edit");
        }
    }
    fn chunk_data_load(&self, _: &dyn ChunkDiskHost, event: &mut ChunkDataLoadEvent<'_>) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let value = event.data.get(&VALUE).unwrap();
        event.data.set(&VALUE, value + 1).unwrap();
    }
}
fn with_listener(apply: impl FnOnce(Arc<AtomicUsize>)) {
    let calls = Arc::new(AtomicUsize::new(0));
    testing::with_dispatch(
        vec![ModEntry::new(
            "disk_fixture",
            Box::new(Listener {
                calls: calls.clone(),
                panic_after_edit: false,
            }),
        )]
        .into(),
        || apply(calls),
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
    with_listener(|calls| {
        let mut live = store(7);
        let job = ChunkDiskJob::capture(&context(ChunkDiskPhase::Save), &live).unwrap();
        VALUE.set(&mut live, Holder::Level, 9).unwrap();
        assert_eq!(read(&run(job)), 8);
        assert_eq!(VALUE.get(&mut live, Holder::Level).unwrap(), 9);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    });
}

#[test]
fn load_disk_8_callback_increment_publishes_9_once() {
    with_listener(|calls| {
        let job = ChunkDiskJob::capture(&context(ChunkDiskPhase::Load), &store(8)).unwrap();
        assert_eq!(read(&run(job)), 9);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    });
}

#[test]
fn a_panicking_disk_listener_keeps_completed_edits_and_only_disables_itself() {
    let broken = Arc::new(AtomicUsize::new(0));
    let healthy = Arc::new(AtomicUsize::new(0));
    testing::with_dispatch(
        vec![
            ModEntry::new(
                "disk_panic_fixture",
                Box::new(Listener {
                    calls: broken.clone(),
                    panic_after_edit: true,
                }),
            ),
            ModEntry::new(
                "healthy_disk_fixture",
                Box::new(Listener {
                    calls: healthy.clone(),
                    panic_after_edit: false,
                }),
            ),
        ]
        .into(),
        || {
            let context = context(ChunkDiskPhase::Save);
            for expected in [9, 8] {
                let job = ChunkDiskJob::capture(&context, &store(7)).unwrap();
                assert_eq!(read(&run(job)), expected);
            }
        },
    );
    assert_eq!(broken.load(Ordering::Relaxed), 1);
    assert_eq!(healthy.load(Ordering::Relaxed), 2);
    let notices = crate::callback_owner::take_callback_panic_notices();
    let mut mine = notices
        .iter()
        .filter(|notice| notice.mod_id == "disk_panic_fixture");
    let notice = mine
        .next()
        .expect("the panic goes to the error toast queue");
    assert_eq!(notice.operation, "chunk_data_save");
    assert!(
        notice
            .message
            .contains("fixture unwind after attachment edit")
    );
    assert!(mine.next().is_none());
}

#[test]
fn codec_failures_preserve_the_entry_and_do_not_prevent_future_edits() {
    let broken = AttachmentType {
        codec: Some(sunrise_mod_runtime::attachments::AttachmentCodec {
            encode: |_| Err(AttachmentError::Encode),
            decode: |_| Err(AttachmentError::Decode),
        }),
        ..VALUE
    };
    let mut data = ChunkDiskData::from_nbt(store(7).to_nbt());
    for _ in 0..2 {
        assert_eq!(data.get(&broken).unwrap_err().reason, "Decode");
        assert_eq!(data.set(&broken, 8).unwrap_err().reason, "Encode");
        assert_eq!(data.remove(&broken).unwrap_err().reason, "Decode");
    }
    assert_eq!(data.get(&VALUE).unwrap(), 7);
    data.set(&VALUE, 8).unwrap();
    assert_eq!(read(&data.finish()), 8);
}

#[test]
fn large_and_deep_attachments_follow_the_normal_disk_nbt_path() {
    let mut value = Tag::ByteArray(vec![0x3a; 1024 * 1024]);
    for _ in 0..70 {
        value = Tag::List(vec![value]);
    }
    let entries = Compound::from([(
        "extension".into(),
        Tag::compound([(
            "unknown",
            Tag::compound([("value", Tag::compound([("data", value)]))]),
        )]),
    )]);
    let original = NamedTag {
        name: "".into(),
        tag: Tag::compound([(ATTACHMENTS_KEY, Tag::Compound(entries.clone()))]),
    };
    let mut data = ChunkDiskData::from_nbt(entries);
    data.set(&VALUE, 8).unwrap();
    let root = NamedTag {
        name: "".into(),
        tag: Tag::compound([(ATTACHMENTS_KEY, Tag::Compound(data.finish()))]),
    };
    let disk = write_named(&root, Limits::saved()).unwrap();
    let (decoded, _) = read_named(&disk, Limits::saved()).unwrap();
    assert_eq!(
        decoded
            .tag
            .get(ATTACHMENTS_KEY)
            .and_then(|tag| tag.get("extension")),
        original
            .tag
            .get(ATTACHMENTS_KEY)
            .and_then(|tag| tag.get("extension")),
    );
}

#[test]
fn overlong_codec_names_and_strings_are_written_empty_by_disk_nbt() {
    let long_string = AttachmentType {
        codec: Some(sunrise_mod_runtime::attachments::AttachmentCodec {
            encode: |_| {
                let long = "界".repeat(21_846);
                Ok(Compound::from([(
                    long.as_str().into(),
                    Tag::String(long.as_str().into()),
                )]))
            },
            decode: |_| Ok(7),
        }),
        ..VALUE
    };
    let mut data = ChunkDiskData::from_nbt(store(7).to_nbt());
    data.set(&long_string, 8).unwrap();
    let root = NamedTag {
        name: "".into(),
        tag: Tag::Compound(data.finish()),
    };
    let disk = write_named(&root, Limits::saved()).unwrap();
    let (decoded, _) = read_named(&disk, Limits::saved()).unwrap();
    assert_eq!(
        decoded
            .tag
            .get("disk_fixture")
            .and_then(|ns| ns.get("value"))
            .and_then(|entry| entry.get("value"))
            .and_then(|value| value.get("")),
        Some(&Tag::String("".into())),
    );
}

#[test]
fn malformed_entries_are_skipped_without_dropping_valid_siblings() {
    let opaque = Tag::compound([("value", Tag::compound([("opaque", Tag::Int(7))]))]);
    let mut data = ChunkDiskData::from_nbt(Compound::from([
        ("bad_namespace".into(), Tag::Int(1)),
        (
            "extension".into(),
            Tag::compound([
                ("not_an_envelope", Tag::Int(2)),
                ("missing_value", Tag::compound([("other", Tag::Int(3))])),
                ("wrong_value_type", Tag::compound([("value", Tag::Int(4))])),
                ("unknown", opaque.clone()),
            ]),
        ),
    ]));
    data.set(&VALUE, 8).unwrap();
    let entries = data.finish();
    assert!(!entries.contains_key(&"bad_namespace".into()));
    assert_eq!(
        entries.get(&"extension".into()),
        Some(&Tag::compound([("unknown", opaque)]))
    );
    assert_eq!(read(&entries), 8);
}

#[test]
fn zero_subscribers_over_100_saves_creates_no_jobs_or_callbacks() {
    testing::with_dispatch(Vec::new().into(), || {
        let snapshot = store(7);
        let context = context(ChunkDiskPhase::Save);
        for _ in 0..100 {
            assert!(ChunkDiskJob::capture(&context, &snapshot).is_none());
        }
    });
}

#[test]
fn type_without_persistence_codec_refuses_naming_type_and_capability() {
    let unsaved = AttachmentType {
        codec: None,
        ..VALUE
    };
    let mut data = ChunkDiskData::from_nbt(store(7).to_nbt());
    let refusal = data.get(&unsaved).unwrap_err();
    assert_eq!(refusal.key, "disk_fixture:value");
    assert_eq!(refusal.reason, "missing persistence codec");
    assert_eq!(read(&data.finish()), 7);
}
