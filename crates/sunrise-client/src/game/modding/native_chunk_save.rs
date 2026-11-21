//! Server capture seams. Disk callbacks run only in the persistence owner.
use super::*;
use sunrise_client::chunk::Chunk;
use sunrise_client::streaming::{ProtoSave, ProtoSnapshot};

impl Game<'_> {
    /// Bind the existing world's identity and immutable resources, before workers start.
    pub(super) fn bind_chunk_disk_worker(
        &self,
        save: &sunrise_client::save::SaveDir,
        dim: super::Dim,
    ) {
        if self.options.execution_role.is_client()
            || (!mod_api::__private::listened(mod_api::HookId::ChunkDataLoad)
                && !mod_api::__private::listened(mod_api::HookId::ChunkDataSave))
        {
            return;
        }
        if let (Some(world), Some(resources)) = (
            self.native_lifecycle.epoch(),
            sunrise_world::native_worldgen_hooks::context(),
        ) {
            save.set_chunk_disk_context(
                world,
                mod_api::Identifier::parse(dim.id()).expect("registered dimension"),
                resources,
            );
        }
    }

    /// Observe the exact full-chunk save copies on the server thread.
    pub(super) fn native_full_chunk_snapshots(&mut self, chunks: &[Arc<Chunk>]) {
        if self.remote.is_some() || mod_api::__private::mods().is_empty() {
            return;
        }
        for chunk in chunks {
            self.native_full_chunk_snapshot(chunk);
        }
    }

    pub(super) fn native_late_full_chunk_snapshot(&mut self, chunk: &Chunk) {
        if self.remote.is_none() && !mod_api::__private::mods().is_empty() {
            self.native_full_chunk_snapshot(chunk);
        }
    }

    fn native_full_chunk_snapshot(&mut self, _chunk: &Chunk) {
        self.bind_chunk_disk_worker(&self.save, self.dim);
    }

    /// The proto stays checked out until its immutable save record is submitted.
    pub(super) fn native_proto_snapshots(&mut self, protos: &[ProtoSave]) {
        if self.remote.is_some() || mod_api::__private::mods().is_empty() {
            return;
        }
        let access = self.streaming.proto_access();
        let dimension = self.dim.id();
        for save in protos {
            let Some(status) = access.status(&save.proto) else {
                continue;
            };
            self.native_proto_snapshot(ProtoSnapshot {
                dimension,
                pos: [save.pos.0, save.pos.1],
                proto: &save.proto,
                access: access.as_ref(),
                status,
            });
        }
    }

    fn native_proto_snapshot(&mut self, _snapshot: ProtoSnapshot<'_>) {
        self.bind_chunk_disk_worker(&self.save, self.dim);
    }
}

impl Game<'_> {
    /// Keep failed snapshots eligible for explicit/unload saves, not another eager attempt
    /// of the same data. As in `ChunkMap.save`, an eager attempt consumes the edit mark.
    pub(in crate::game) fn requeue_failed_chunk_saves(&mut self) {
        for key in self.save.take_failed_chunk_saves() {
            if self.world.chunks.contains_key(&key) {
                self.world.modified.retain_refused_save(key);
            }
        }
    }

    /// With a `chunk_data_save` listener, saves wait while an action savepoint
    /// is open, so tentative state is never captured: unloads, and the plan
    /// that causes them, are deferred until it closes (the design's "defer
    /// unload when the gate refuses"). Never defers without a listener.
    pub(super) fn defer_chunk_disk_save(&mut self) -> bool {
        if self.options.execution_role.is_client() {
            return false;
        }
        self.requeue_failed_chunk_saves();
        mod_api::__private::listened(mod_api::HookId::ChunkDataSave)
            && Self::admit_entity_save().is_err()
    }

    /// `ChunkMap.saveChunksEagerly`: snapshot only while the tick and writer
    /// have room. NBT encoding, compression and durable writes stay on workers.
    pub(super) fn save_chunks_eagerly(&mut self) {
        self.save_chunks_eagerly_at(Instant::now());
    }

    fn save_chunks_eagerly_at(&mut self, now: Instant) {
        // No world open (`world_root` is set by `finish_open_world`): the save
        // is `Game::new`'s placeholder under the working directory.
        if self.world.carrier_fences.is_broken()
            || self.remote.is_some()
            || self.world_root.as_os_str().is_empty()
            || self.agent.hold_server
            || self.clock.sprinting()
            || self
                .server_lists
                .as_ref()
                .is_some_and(|lists| !lists.lock().unwrap().autosave)
        {
            return;
        }
        if self.defer_chunk_disk_save() {
            return;
        }
        let deadline = self
            .server_tick_deadline
            .unwrap_or_else(|| Instant::now() + self.clock.tick_duration());
        let mut modified = std::mem::take(&mut self.world.modified);
        let mut refused = Vec::new();
        modified.save_eagerly(
            now,
            || Instant::now() < deadline && self.save.active_chunk_writes() < 128,
            |key| self.world.chunks.contains_key(&key),
            |key| {
                // `saveChunkIfNeeded` saves only a LevelChunk, so a lit one; a
                // column still waiting for its light keeps its queue entry.
                let Some(chunk) = self.world.chunks.get(&key).filter(|c| c.lit) else {
                    return false;
                };
                let chunks = self.with_entities([chunk]);
                if let Err(e) =
                    self.save
                        .save_later(&chunks, self.pack.assets.registry, world_biomes!(self))
                {
                    log::warn!("Sunrise: failed to save changed chunks: {e}");
                    refused.push(key);
                }
                true
            },
        );
        for key in refused {
            modified.retain_refused_save(key);
        }
        self.world.modified = modified;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::native_entity_refs::tests::with_game;
    use std::time::Duration;
    use sunrise_client::save::{LevelShape, RecordSaveFailures, SaveDir};
    use sunrise_core::nbt::{DISK_WRITE_NESTING_GUARD, Tag};

    #[test]
    #[ignore = "daily"]
    fn eager_ticks_do_not_retry_a_refused_snapshot_without_an_edit() {
        let dir = tempfile::tempdir().unwrap();
        mod_platform::testing::with_mods(Vec::new().into(), || {
            with_game(|game| {
                game.world_root = dir.path().to_path_buf();
                game.save = Arc::new(SaveDir::new(
                    game.world_root.clone(),
                    LevelShape::OVERWORLD,
                    Some(Default::default()),
                ));
                let key = (2, 0);
                let mut chunk = Chunk::with_layout(key.0, key.1, game.world.layout());
                chunk.lit = true;
                let invalid = (0..=DISK_WRITE_NESTING_GUARD)
                    .fold(Tag::Int(0), |nested, _| Tag::List(vec![nested]));
                let extra = Arc::make_mut(chunk.saved.get_or_insert_with(Default::default));
                extra.root.insert("test:invalid".into(), invalid);
                extra.root.insert("test:attempt".into(), Tag::Int(1));
                game.world.chunks.insert(key, Arc::new(chunk));
                game.world.modified.insert(key);

                let now = Instant::now();
                game.save_chunks_eagerly_at(now);
                let refused = |game: &Game<'_>, attempted| {
                    assert!(game.save.flush().unwrap_err().is::<RecordSaveFailures>());
                    let saved = game.save.raw_chunk(key.0, key.1).unwrap();
                    assert_eq!(saved.tag.get("test:attempt"), Some(&Tag::Int(attempted)));
                };
                refused(game, 1);

                // Advance past the cooldown without sleeping. An out-of-band marker
                // tells us if the unencodable chunk was captured a second time.
                let chunk = Arc::make_mut(game.world.chunks.get_mut(&key).unwrap());
                Arc::make_mut(chunk.saved.as_mut().unwrap())
                    .root
                    .insert("test:attempt".into(), Tag::Int(2));
                for elapsed in [11, 22, 33] {
                    game.save_chunks_eagerly_at(now + Duration::from_secs(elapsed));
                    refused(game, 1);
                }
                game.save_level(false).unwrap();
                refused(game, 1);

                game.world.modified.insert(key);
                game.save_chunks_eagerly_at(now + Duration::from_secs(44));
                refused(game, 2);

                let chunk = Arc::make_mut(game.world.chunks.get_mut(&key).unwrap());
                Arc::make_mut(chunk.saved.as_mut().unwrap())
                    .root
                    .insert("test:attempt".into(), Tag::Int(3));
                assert!(game.save_game().unwrap_err().is::<RecordSaveFailures>());
                refused(game, 3);
            });
        });
    }

    #[test]
    #[ignore = "daily"]
    fn shutdown_saves_a_refused_edit_after_the_chunk_reloads() {
        use mod_api::{
            AttachmentCodec, AttachmentError, AttachmentType, ChunkDataSaveEvent, ChunkDiskHost,
            Hooks, ModEntry,
        };
        use std::sync::{Mutex, mpsc};
        use sunrise_world::native_load_context::{DataLoadOrigin, LoadContext, LoadInputs};

        const KEY: (i32, i32) = (2, 0);
        struct RefuseOnce(Arc<Mutex<Option<mpsc::Receiver<()>>>>);
        #[mod_api::__subscriptions]
        impl Hooks for RefuseOnce {
            fn chunk_data_save(
                &self,
                host: &dyn ChunkDiskHost,
                event: &mut ChunkDataSaveEvent<'_>,
            ) {
                if host.job().pos != [KEY.0, KEY.1] {
                    return;
                }
                let gate = self.0.lock().unwrap().take();
                if let Some(gate) = gate {
                    gate.recv_timeout(Duration::from_secs(5)).unwrap();
                    let invalid = AttachmentType {
                        namespace: "reload_refusal",
                        name: "bad",
                        default: || 0,
                        codec: Some(AttachmentCodec {
                            encode: |_| Err(AttachmentError::Encode),
                            decode: |_| Ok(0),
                        }),
                        copy_on_death: false,
                        sync: false,
                    };
                    let _ = event.data.set(&invalid, 1);
                }
            }
        }

        let gate = Arc::new(Mutex::new(None));
        let mods = vec![ModEntry::new(
            "reload_refusal",
            Box::new(RefuseOnce(gate.clone())),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_game(|game| {
                let dir = tempfile::tempdir().unwrap();
                game.world_root = dir.path().to_path_buf();
                game.session_lock =
                    Some(sunrise_client::save::SessionLock::acquire(dir.path()).unwrap());
                game.world_adopted = true;
                let open = || {
                    SaveDir::new(
                        dir.path().to_path_buf(),
                        LevelShape::OVERWORLD,
                        Some(Default::default()),
                    )
                };
                game.save = Arc::new(open());
                let load = LoadContext::new(
                    DataLoadOrigin::ServerDataLoad,
                    Arc::new(LoadInputs::new(
                        Default::default(),
                        game.world_root.join("settings"),
                    )),
                    [],
                );
                game.save.set_chunk_disk_context(
                    mod_api::WorldEpoch::new(17),
                    mod_api::Identifier::parse("minecraft:overworld").unwrap(),
                    Arc::new(load.worldgen_context([])),
                );
                let registry = game.pack.assets.registry;
                let stone = registry.default_state("minecraft:stone").unwrap();
                let gold = registry.default_state("minecraft:gold_block").unwrap();
                let mut chunk = Chunk::with_layout(KEY.0, KEY.1, game.world.layout());
                chunk.lit = true;
                chunk.set(1, 64, 1, stone.0);
                game.save
                    .save(&[Arc::new(chunk.clone())], registry, world_biomes!(game))
                    .unwrap();

                let (allow_refusal, wait_for_unload) = mpsc::channel();
                *gate.lock().unwrap() = Some(wait_for_unload);
                chunk.set(1, 64, 1, gold.0);
                game.world.chunks.insert(KEY, Arc::new(chunk));
                game.world.modified.insert(KEY);
                game.save_chunks_eagerly_at(Instant::now());
                assert!(!game.world.modified.contains(&KEY));
                assert!(game.world.unload_chunk(KEY).is_some());
                allow_refusal.send(()).unwrap();
                assert!(game.save.flush().unwrap_err().is::<RecordSaveFailures>());
                game.requeue_failed_chunk_saves();

                let loaded = game
                    .save
                    .load(KEY.0, KEY.1, registry, world_biomes!(game))
                    .unwrap();
                assert_eq!(loaded.block(1, 64, 1), gold);
                game.world.chunks.insert(KEY, Arc::new(loaded));
                game.world.loaded_chunks.push((KEY.0, KEY.1, false));
                game.native_chunks_loaded();
                game.run_server_lifecycle(false, |_, began| {
                    *began = true;
                    Ok(())
                })
                .unwrap();

                let disk = open()
                    .load(KEY.0, KEY.1, registry, world_biomes!(game))
                    .unwrap();
                assert_eq!(disk.block(1, 64, 1), gold);
            });
        });
    }

    #[test]
    #[ignore = "daily"]
    fn an_unloaded_refusal_cannot_hide_a_later_edit_from_eager_saves() {
        let dir = tempfile::tempdir().unwrap();
        mod_platform::testing::with_mods(Vec::new().into(), || {
            with_game(|game| {
                game.world_root = dir.path().to_path_buf();
                let open = || {
                    SaveDir::new(
                        dir.path().to_path_buf(),
                        LevelShape::OVERWORLD,
                        Some(Default::default()),
                    )
                };
                game.save = Arc::new(open());
                let key = (2, 0);
                let registry = game.pack.assets.registry;
                let mut unloaded = Chunk::with_layout(key.0, key.1, game.world.layout());
                let invalid = (0..=DISK_WRITE_NESTING_GUARD)
                    .fold(Tag::Int(0), |nested, _| Tag::List(vec![nested]));
                Arc::make_mut(unloaded.saved.get_or_insert_with(Default::default))
                    .root
                    .insert("test:invalid".into(), invalid);
                game.save
                    .save_later(&[Arc::new(unloaded)], registry, world_biomes!(game))
                    .unwrap();
                assert!(game.save.flush().unwrap_err().is::<RecordSaveFailures>());
                game.requeue_failed_chunk_saves();
                assert!(!game.world.modified.contains(&key));

                // Even a stale edit mark visited after unload must not poison the queue.
                game.world.modified.insert(key);
                game.save_chunks_eagerly();
                assert!(!game.world.modified.contains(&key));

                let mut loaded = Chunk::with_layout(key.0, key.1, game.world.layout());
                loaded.lit = true;
                let stone = registry.default_state("minecraft:stone").unwrap();
                loaded.set(1, 64, 1, stone.0);
                game.world.chunks.insert(key, Arc::new(loaded));
                game.world.modified.insert(key);
                game.save_chunks_eagerly();
                game.save.flush().unwrap();
                let disk = open()
                    .load(key.0, key.1, registry, world_biomes!(game))
                    .unwrap();
                assert_eq!(disk.block(1, 64, 1), stone);
            });
        });
    }
}
