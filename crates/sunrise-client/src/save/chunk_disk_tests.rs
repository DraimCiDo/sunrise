//! Real region-file checks for the chunk worker's attachment boundary.
use super::*;
use mod_api::{ChunkDataLoadEvent, ChunkDataSaveEvent, ChunkDiskHost, Hooks};
use std::sync::atomic::AtomicUsize;
mod fixture;
mod writer_stack;

#[derive(Default)]
struct Listener {
    saves: Arc<AtomicUsize>,
    loads: Arc<AtomicUsize>,
}
#[mod_api::sunrise_mod]
impl Hooks for Listener {
    fn chunk_data_save(&self, host: &dyn ChunkDiskHost, event: &mut ChunkDataSaveEvent<'_>) {
        self.saves.fetch_add(1, Ordering::Relaxed);
        let names: &[String] = host.registry("minecraft:block").unwrap();
        assert_eq!(names, ["minecraft:stone", "minecraft:dirt"]);
        fixture::save(event.data);
    }
    fn chunk_data_load(&self, host: &dyn ChunkDiskHost, event: &mut ChunkDataLoadEvent<'_>) {
        self.loads.fetch_add(1, Ordering::Relaxed);
        fixture::load(host, event.data);
    }
}
fn context(save: &SaveDir) {
    use sunrise_world::native_load_context::{DataLoadOrigin, LoadContext, LoadInputs};
    let load = LoadContext::new(
        DataLoadOrigin::ServerDataLoad,
        Arc::new(LoadInputs::new(
            BTreeMap::from([(
                "minecraft:block".into(),
                vec!["minecraft:stone".into(), "minecraft:dirt".into()],
            )]),
            save.root.join("settings"),
        )),
        [],
    );
    save.set_chunk_disk_context(
        mod_api::WorldEpoch::new(17),
        mod_api::Identifier::parse("minecraft:overworld").unwrap(),
        Arc::new(load.worldgen_context([])),
    );
}
fn chunk(value: i32) -> Chunk {
    let mut chunk = Chunk::empty(2, -3);
    let mut extras = ChunkExtras::default();
    fixture::VALUE
        .set(&mut extras.attachments, mod_api::Holder::Level, value)
        .unwrap();
    chunk.saved = Some(Arc::new(extras));
    chunk
}
fn value(chunk: &mut Chunk) -> i32 {
    fixture::VALUE
        .get(
            &mut Arc::make_mut(chunk.saved.as_mut().unwrap()).attachments,
            mod_api::Holder::Level,
        )
        .unwrap()
}
fn root() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "sunrise-chunk-disk-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
fn with_listener(run: impl FnOnce(Arc<AtomicUsize>, Arc<AtomicUsize>)) {
    let saves = Arc::new(AtomicUsize::new(0));
    let loads = Arc::new(AtomicUsize::new(0));
    let mods = vec![mod_api::ModEntry::new(
        "chunk_disk_fixture",
        Box::new(Listener {
            saves: saves.clone(),
            loads: loads.clone(),
        }),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || run(saves, loads));
}

#[test]
fn capture_7_callback_8_live_9_round_trips_through_region_writer() {
    let (registry, biomes) = super::loot_tests::fixture();
    let registry = Box::leak(Box::new(registry));
    let biomes = Arc::new(biomes);
    let path = root();
    with_listener(|saves, _| {
        let save = SaveDir::new(
            path.clone(),
            LevelShape::OVERWORLD,
            Some(Default::default()),
        );
        context(&save);
        let mut live = chunk(7);
        save.save_later(&[Arc::new(live.clone())], registry, &biomes)
            .unwrap();
        fixture::VALUE
            .set(
                &mut Arc::make_mut(live.saved.as_mut().unwrap()).attachments,
                mod_api::Holder::Level,
                9,
            )
            .unwrap();
        save.flush().unwrap();
        assert_eq!(saves.load(Ordering::Relaxed), 1);
        assert_eq!(value(&mut live), 9);
        let disk = SaveDir::new(
            path.clone(),
            LevelShape::OVERWORLD,
            Some(Default::default()),
        );
        assert_eq!(value(&mut disk.load(2, -3, registry, &biomes).unwrap()), 8);
    });
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn load_disk_8_callback_increment_publishes_9_once_through_decode() {
    let (registry, biomes) = super::loot_tests::fixture();
    let path = root();
    mod_platform::testing::with_mods(Vec::new().into(), || {
        SaveDir::new(
            path.clone(),
            LevelShape::OVERWORLD,
            Some(Default::default()),
        )
        .save(&[Arc::new(chunk(8))], &registry, &biomes)
        .unwrap();
    });
    with_listener(|_, loads| {
        let save = SaveDir::new(
            path.clone(),
            LevelShape::OVERWORLD,
            Some(Default::default()),
        );
        context(&save);
        assert_eq!(value(&mut save.load(2, -3, &registry, &biomes).unwrap()), 9);
        assert_eq!(loads.load(Ordering::Relaxed), 1);
    });
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn full_and_proto_writer_records_each_invoke_once_before_encode() {
    let (registry, biomes) = super::loot_tests::fixture();
    let registry = Box::leak(Box::new(registry));
    let biomes = Arc::new(biomes);
    let path = root();
    with_listener(|saves, _| {
        let save = SaveDir::new(
            path.clone(),
            LevelShape::OVERWORLD,
            Some(Default::default()),
        );
        context(&save);
        save.save_later(&[Arc::new(chunk(7))], registry, &biomes)
            .unwrap();
        save.save_protos(
            vec![SavedProto {
                column: chunk(7),
                decorated: true,
                block_entities: Vec::new(),
                entities: Vec::new(),
            }],
            registry,
            &biomes,
        )
        .unwrap();
        save.flush().unwrap();
        assert_eq!(saves.load(Ordering::Relaxed), 2);
        assert_eq!(
            value(
                &mut SaveDir::new(path.clone(), LevelShape::OVERWORLD, None)
                    .load_proto(2, -3, registry, &biomes)
                    .unwrap()
                    .column
            ),
            8
        );
    });
    fs::remove_dir_all(path).unwrap();
}

/// Records what each save callback saw, in the order the writer ran them.
// Its own module: `sunrise_mod` defines one `hooks` function per module.
mod echo {
    use super::*;
    #[derive(Default)]
    pub struct Echo {
        pub seen: Arc<Mutex<Vec<(mod_api::ChunkDiskKind, i32)>>>,
    }
    #[mod_api::sunrise_mod]
    impl Hooks for Echo {
        fn chunk_data_save(&self, host: &dyn ChunkDiskHost, event: &mut ChunkDataSaveEvent<'_>) {
            // A worker callback has no live world: the old handle's path refuses.
            assert!(host.backend().is_none());
            let value = event.data.get(&fixture::VALUE).unwrap();
            self.seen.lock().unwrap().push((host.job().kind, value));
        }
    }
}
use echo::Echo;
fn with_echo(run: impl FnOnce(Arc<Mutex<Vec<(mod_api::ChunkDiskKind, i32)>>>)) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mods = vec![mod_api::ModEntry::new(
        "chunk_disk_fixture",
        Box::new(Echo { seen: seen.clone() }),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || run(seen));
}
fn disk_value(path: &Path, registry: &BlockRegistry, biomes: &Biomes) -> Option<i32> {
    SaveDir::new(
        path.to_path_buf(),
        LevelShape::OVERWORLD,
        Some(Default::default()),
    )
    .load(2, -3, registry, biomes)
    .map(|mut chunk| value(&mut chunk))
}

#[test]
fn revisions_10_then_11_queued_together_leave_disk_at_11() {
    let (registry, biomes) = super::loot_tests::fixture();
    let registry = Box::leak(Box::new(registry));
    let biomes = Arc::new(biomes);
    let path = root();
    with_echo(|seen| {
        let save = SaveDir::new(
            path.clone(),
            LevelShape::OVERWORLD,
            Some(Default::default()),
        );
        context(&save);
        save.save_later(&[Arc::new(chunk(10))], registry, &biomes)
            .unwrap();
        save.save_later(&[Arc::new(chunk(11))], registry, &biomes)
            .unwrap();
        save.flush().unwrap();
        // Each captured record had its callback, whether or not its write was
        // coalesced away; only the newest revision is on disk.
        let seen: Vec<i32> = seen.lock().unwrap().iter().map(|(_, v)| *v).collect();
        assert_eq!(seen, [10, 11]);
        assert_eq!(disk_value(&path, registry, &biomes), Some(11));
    });
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn every_save_kind_runs_exactly_one_callback_per_captured_record() {
    let (registry, biomes) = super::loot_tests::fixture();
    let registry = Box::leak(Box::new(registry));
    let biomes = Arc::new(biomes);
    let path = root();
    with_echo(|seen| {
        let save = SaveDir::new(
            path.clone(),
            LevelShape::OVERWORLD,
            Some(Default::default()),
        );
        context(&save);
        // Asynchronous full saves: explicit, forced, late and unload all queue this way.
        save.save_later(&[Arc::new(chunk(1))], registry, &biomes)
            .unwrap();
        save.save_later(&[Arc::new(chunk(2))], registry, &biomes)
            .unwrap();
        save.flush().unwrap();
        // The synchronous route used by closing and forced saves.
        save.save(&[Arc::new(chunk(3))], registry, &biomes).unwrap();
        // Proto chunks.
        save.save_protos(
            vec![SavedProto {
                column: chunk(4),
                decorated: true,
                block_entities: Vec::new(),
                entities: Vec::new(),
            }],
            registry,
            &biomes,
        )
        .unwrap();
        save.flush().unwrap();
        use mod_api::ChunkDiskKind::{Full, Proto};
        assert_eq!(
            *seen.lock().unwrap(),
            [(Full, 1), (Full, 2), (Full, 3), (Proto, 4)]
        );
    });
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn unbound_chunk_context_does_not_prevent_sync_or_async_saves() {
    let (registry, biomes) = super::loot_tests::fixture();
    let registry = Box::leak(Box::new(registry));
    let biomes = Arc::new(biomes);
    let path = root();
    with_echo(|seen| {
        let mut save = SaveDir::new(
            path.clone(),
            LevelShape::OVERWORLD,
            Some(Default::default()),
        );
        save.enable_trusted_fluid_load();
        save.save_later(&[Arc::new(chunk(7))], registry, &biomes)
            .unwrap();
        save.flush().unwrap();
        assert_eq!(disk_value(&path, registry, &biomes), Some(7));
        save.save(&[Arc::new(chunk(8))], registry, &biomes).unwrap();
        assert_eq!(disk_value(&path, registry, &biomes), Some(8));
        assert!(seen.lock().unwrap().is_empty());
    });
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn unload_then_reload_reads_saved_8_in_a_fresh_holder() {
    let (registry, biomes) = super::loot_tests::fixture();
    let registry = Box::leak(Box::new(registry));
    let biomes = Arc::new(biomes);
    let path = root();
    with_listener(|_, _| {
        let save = SaveDir::new(
            path.clone(),
            LevelShape::OVERWORLD,
            Some(Default::default()),
        );
        context(&save);
        save.save_later(&[Arc::new(chunk(7))], registry, &biomes)
            .unwrap();
        save.flush().unwrap();
    });
    // No listener on reload: the stored value is exactly what the save callback wrote.
    mod_platform::testing::with_mods(Vec::new().into(), || {
        assert_eq!(disk_value(&path, registry, &biomes), Some(8));
    });
    fs::remove_dir_all(path).unwrap();
}

#[cfg(unix)]
#[test]
fn a_failed_disk_write_does_not_stop_healthy_regions_in_the_same_batch() {
    use std::os::unix::fs::PermissionsExt;
    let (registry, biomes) = super::loot_tests::fixture();
    let registry = Box::leak(Box::new(registry));
    let biomes = Arc::new(biomes);
    let path = root();
    mod_platform::testing::with_mods(Vec::new().into(), || {
        SaveDir::new(
            path.clone(),
            LevelShape::OVERWORLD,
            Some(Default::default()),
        )
        .save(&[Arc::new(chunk(7))], registry, &biomes)
        .unwrap();
    });
    let region = path.join("region/r.0.-1.mca");
    let writable = fs::metadata(&region).unwrap().permissions();
    fs::set_permissions(&region, fs::Permissions::from_mode(0o444)).unwrap();
    with_listener(|saves, _| {
        let save = SaveDir::new(
            path.clone(),
            LevelShape::OVERWORLD,
            Some(Default::default()),
        );
        context(&save);
        let mut healthy = chunk(7);
        healthy.x = 34;
        save.save_later(&[Arc::new(chunk(7)), Arc::new(healthy)], registry, &biomes)
            .unwrap();
        // The close path reports the failure instead of treating the save as durable.
        assert!(save.flush().is_err());
        assert_eq!(saves.load(Ordering::Relaxed), 2);
        // The unwritten snapshot (8) stays readable for recovery. Read it with no
        // listener installed, so no load callback adds to it.
        mod_platform::testing::with_mods(Vec::new().into(), || {
            let mut recovered = save.load(2, -3, registry, &biomes).unwrap();
            assert_eq!(value(&mut recovered), 8);
        });
    });
    fs::set_permissions(&region, writable).unwrap();
    // The old disk record is untouched.
    mod_platform::testing::with_mods(Vec::new().into(), || {
        assert_eq!(disk_value(&path, registry, &biomes), Some(7));
        let disk = SaveDir::new(path.clone(), LevelShape::OVERWORLD, None);
        assert_eq!(value(&mut disk.load(34, -3, registry, &biomes).unwrap()), 8);
    });
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn load_panic_keeps_the_chunk_loadable_and_its_save_listener_enabled() {
    let (registry, biomes) = super::loot_tests::fixture();
    let registry = Box::leak(Box::new(registry));
    let biomes = Arc::new(biomes);
    for sync in [true, false] {
        let path = root();
        mod_platform::testing::with_mods(Vec::new().into(), || {
            SaveDir::new(
                path.clone(),
                LevelShape::OVERWORLD,
                Some(Default::default()),
            )
            .save(&[Arc::new(chunk(8))], registry, &biomes)
            .unwrap();
        });
        let saves = Arc::new(AtomicUsize::new(0));
        let loads = Arc::new(AtomicUsize::new(0));
        let mods = vec![mod_api::ModEntry::new(
            "chunk_disk_fixture",
            Box::new(breaker::Breaker {
                saves: saves.clone(),
                loads: loads.clone(),
            }),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            let save = SaveDir::new(
                path.clone(),
                LevelShape::OVERWORLD,
                Some(Default::default()),
            );
            context(&save);
            let mut loaded = save
                .load(2, -3, registry, &biomes)
                .expect("the chunk loads");
            assert_eq!(value(&mut loaded), 8);
            let mut loaded_again = save
                .load(2, -3, registry, &biomes)
                .expect("the chunk reloads");
            assert_eq!(value(&mut loaded_again), 8);
            let chunks = [Arc::new(loaded)];
            if sync {
                save.save(&chunks, registry, &biomes).unwrap();
            } else {
                save.save_later(&chunks, registry, &biomes).unwrap();
                save.flush().unwrap();
            }
            assert_eq!(disk_value(&path, registry, &biomes), Some(9));
        });
        assert_eq!(loads.load(Ordering::Relaxed), 1);
        assert_eq!(saves.load(Ordering::Relaxed), 1);
        fs::remove_dir_all(path).unwrap();
    }
}

// Its own module: `sunrise_mod` defines one `hooks` function per module.
mod breaker {
    use super::*;
    #[derive(Default)]
    pub struct Breaker {
        pub saves: Arc<AtomicUsize>,
        pub loads: Arc<AtomicUsize>,
    }
    #[mod_api::sunrise_mod]
    impl Hooks for Breaker {
        fn chunk_data_load(&self, _: &dyn ChunkDiskHost, _: &mut ChunkDataLoadEvent<'_>) {
            self.loads.fetch_add(1, Ordering::Relaxed);
            panic!("load listener fails");
        }
        fn chunk_data_save(&self, _: &dyn ChunkDiskHost, event: &mut ChunkDataSaveEvent<'_>) {
            self.saves.fetch_add(1, Ordering::Relaxed);
            event.data.set(&fixture::VALUE, 9).unwrap();
        }
    }
}
