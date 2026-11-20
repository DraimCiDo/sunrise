//! The game as `tools/hot_paths.py` measures it (docs/hot-path-trends.md). Without
//! `SUNRISE_HOT_PATHS_MODS=native` it starts exactly as `sunrise-game` does, with no mod. With it,
//! it is what the launcher's generated `main` builds for one native mod: `hot_paths_listener`
//! listens to the entity tick hooks and does nothing in them, so the benchmark's hook workload
//! measures a dispatch that reaches a listener. That run needs a
//! `sunrise-native-mods/hot_paths_listener` package beside the executable, which the tool writes.
//!
//! A crate and not a feature of `sunrise-game`: the game players get links no mod.
#![forbid(unsafe_code)]
use mod_api::__private::native::BuiltMod;
use mod_api::Side;
use mod_api::{
    BlockEntityPart, BlockEntityTickEvent, BlockEntityTickSubject, BlockEntityTickViewEvent,
    BlockState, EntityTickListenedEvent, EntityTickPostEvent, EntityTickPreEvent, Hooks, HostSide,
    ModEntry, Registrar, RegistrationContext, ServerHost,
};

const ID: &str = "hot_paths_listener";
const BUILT: &[BuiltMod] = &[BuiltMod {
    id: ID,
    version: "1.0.0",
    side: Side::Both,
}];

struct Listener;

#[mod_api::__subscriptions]
impl Hooks for Listener {
    fn register(&self, registrar: &mut dyn Registrar, _: &RegistrationContext) {
        match std::env::var("SUNRISE_BLOCK_ENTITY_VIEW_BENCH").as_deref() {
            Ok("view" | "handle") => {
                let kind = registrar.block_entity_type("progress").build();
                registrar
                    .block_entity_part(&kind, ResidentProgress)
                    .unwrap();
            }
            Ok("snapshot") => {
                let kind = registrar.block_entity_type("progress_snapshot").build();
                registrar
                    .block_entity_part(&kind, SnapshotProgress)
                    .unwrap();
            }
            _ => {}
        }
    }

    fn entity_tick_listened(&self, _: HostSide<'_>, event: &mut EntityTickListenedEvent) {
        event.listened = true;
    }
    fn entity_tick_pre(&self, _: HostSide<'_>, _: &mut EntityTickPreEvent<'_>) {}
    fn entity_tick_post(&self, _: HostSide<'_>, _: &mut EntityTickPostEvent<'_>) {}
}

// Read the machine's orientation/state and progress, then advance its menu counter.
fn next_progress(state: BlockState, progress: i32) -> i32 {
    progress + 1 + i32::from(state.ordinal())
}

struct ResidentProgress;

#[mod_api::sunrise_part]
impl mod_api::BlockPart for ResidentProgress {}

#[mod_api::sunrise_part]
impl BlockEntityPart for ResidentProgress {
    fn block_entity_tick_view(&self, event: &mut BlockEntityTickViewEvent<'_>) {
        match &mut event.subject {
            BlockEntityTickSubject::View(view) => {
                let value = next_progress(view.state(), view.data_slots()[0]);
                assert!(view.set_data_slot(0, value));
            }
            BlockEntityTickSubject::Handle { host, entity } => {
                let level = entity.level(&**host).unwrap();
                let pos = entity.position(&**host).unwrap();
                let state = level.block_state(&**host, pos).unwrap();
                let value = next_progress(state, entity.data_slots(&**host).unwrap()[0]);
                assert!(entity.set_data_slot(&mut **host, 0, value).unwrap());
            }
            _ => unreachable!("unrecognized benchmark route"),
        }
    }
}

struct SnapshotProgress;

#[mod_api::sunrise_part]
impl mod_api::BlockPart for SnapshotProgress {}

#[mod_api::sunrise_part]
impl BlockEntityPart for SnapshotProgress {
    fn block_entity_tick(&self, host: &mut dyn ServerHost, event: &mut BlockEntityTickEvent<'_>) {
        let state = host
            .level("minecraft:overworld")
            .unwrap()
            .block_state(host, event.pos)
            .unwrap();
        event.entity.data[0] = next_progress(state, event.entity.data[0]);
    }
}

fn main() {
    sunrise_mod_host::prepare_main_stack();
    let startup = sunrise_client::entry::prepare();
    std::hint::black_box(&sunrise_mod_package::holder::CAPABILITY);
    if std::env::var_os("SUNRISE_HOT_PATHS_MODS").is_some_and(|mods| mods == "native") {
        sunrise_mod_host::main_with(vec![ModEntry::new(ID, Box::new(Listener))], BUILT, startup);
    } else {
        sunrise_mod_host::main_with(Vec::new(), &[], startup);
    }
}
