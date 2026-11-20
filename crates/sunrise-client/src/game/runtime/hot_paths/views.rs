//! One resident machine-progress update at each production mod ticker call.
use super::{super::Game, Recorder, world::FLOOR};
use sunrise_client::{
    Result,
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
        self.hot_level(7, 2)?;
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
