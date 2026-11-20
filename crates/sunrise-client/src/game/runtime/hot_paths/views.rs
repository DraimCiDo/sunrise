//! One resident machine-progress update at each production mod ticker call.
use super::{super::Game, Recorder, world::FLOOR};
use std::sync::Arc;
use sunrise_client::{
    Result,
    chunk::Chunk,
    containers::{BlockEntity, ModBlockEntity, ModBlockEntityFields},
};
use sunrise_core::nbt::Tag;

const MACHINES: u64 = 128;
const TICKS: u64 = 1_000;
const WARMUP: u64 = 20;
const INITIAL_PROGRESS: i32 = 23;

impl Game<'_> {
    pub(super) fn hot_block_entity_view(
        &mut self,
        recorder: &mut Recorder,
        mode: &str,
        listening: bool,
    ) -> Result<()> {
        let kind = match (mode, listening) {
            ("snapshot", true) => "hot_paths_listener:progress_snapshot",
            ("view" | "handle", true) | ("no_listener" | "empty", false) => {
                "hot_paths_listener:progress"
            }
            _ => return Err(format!("invalid block-entity tick benchmark mode: {mode}").into()),
        };
        let name = format!("mod.block_entity_view_v1.{mode}");
        // The component harness skips world open. Bind the same typed state and Part tables
        // a playing server would use before starting the ticker workload.
        self.resolve_parts()?;
        if listening {
            let hook = if mode == "snapshot" {
                mod_api::HookId::BlockEntityTick
            } else {
                mod_api::HookId::BlockEntityTickView
            };
            if mod_api::__private::block_entity_type_key(kind).is_none()
                || !mod_api::__private::block_entity_type_listens(hook, kind)
            {
                return Err(format!("{name}: no ticker Part bound for {kind}").into());
            }
        }
        // This workload calls the ticker directly, so no streaming or forced chunk tickets
        // are needed. Use the normal mod world with the one chunk holding all the machines.
        self.world = Game::world_for_mods(self.res.facts().air, 7);
        self.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        let stone = self
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        let positions: Vec<_> = (0..MACHINES as i32)
            .map(|i| (i % 16, FLOOR + 1, i / 16))
            .collect();
        for &(x, y, z) in &positions {
            self.world.set(x, y, z, stone)?;
            let mut fields = ModBlockEntityFields {
                data: vec![INITIAL_PROGRESS, 7200],
                ..Default::default()
            };
            fields
                .nbt
                .insert("recipe".into(), Tag::String("test:mix".into()));
            fields.nbt.insert("batch".into(), Tag::Int(64));
            self.world.block_entities.insert(
                (x, y, z),
                BlockEntity::Mod(Box::new(ModBlockEntity::new(kind.into(), fields))),
            );
        }
        Self::benchmark_block_entity_tick_handle(mode == "handle");
        let empty = mode == "empty";
        let tick = |game: &mut Self, count| {
            if empty {
                for _ in 0..count {
                    for &pos in &positions {
                        std::hint::black_box(pos);
                    }
                }
            } else {
                for _ in 0..count {
                    for &pos in &positions {
                        game.tick_mod_block_entity(pos);
                    }
                }
            }
        };
        tick(self, WARMUP);
        // Stone has one state (ordinal zero); each callback advances progress by exactly one.
        let expected = INITIAL_PROGRESS
            + if listening {
                (WARMUP + TICKS) as i32
            } else {
                0
            };
        let mut complete = true;
        recorder.measure_observed(
            &name,
            MACHINES * TICKS,
            || {
                tick(self, TICKS);
                self
            },
            |game| {
                let mut sum = 0i32;
                for pos in &positions {
                    if let Some(BlockEntity::Mod(entity)) = game.world.block_entities.get(pos) {
                        sum += entity.data[0];
                        complete &= entity.data == [expected, 7200];
                    } else {
                        complete = false;
                    }
                }
                sum
            },
        )?;
        Self::benchmark_block_entity_tick_handle(false);
        if !complete {
            return Err(
                format!("{name}: not every machine reached expected progress {expected}").into(),
            );
        }
        Ok(())
    }
}
