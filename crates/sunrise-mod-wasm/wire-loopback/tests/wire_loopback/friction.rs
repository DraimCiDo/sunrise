use super::*;
use mod_api::{BlockPart, BlockState, FrictionEvent, LevelHost, LevelSide};

const POS: [i32; 3] = [5, 64, 4];
const BOAT: Option<(u32, &str)> = Some((7, "minecraft:oak_boat"));
thread_local! { static ANSWER: std::cell::Cell<f32> = const { std::cell::Cell::new(0.98) }; }

struct World(BlockState);
impl LevelHost for World {
    fn block_state(&self, pos: [i32; 3]) -> Option<BlockState> {
        assert_eq!(pos, POS);
        Some(self.0)
    }

    fn friction(
        &self,
        pos: [i32; 3],
        state: BlockState,
        entity: Option<(u32, &str)>,
    ) -> Option<f32> {
        assert_eq!(pos, POS);
        assert_eq!(state, self.0);
        assert!(entity.is_none() || entity == BOAT);
        Some(ANSWER.get())
    }
}

struct Surface;
#[mod_api::sunrise_part]
impl BlockPart for Surface {
    fn friction(&self, side: LevelSide<'_>, event: &mut FrictionEvent<'_>) {
        let level = match side {
            LevelSide::Server(level) => {
                assert_eq!(event.entity, BOAT);
                level
            }
            LevelSide::Client(level) => {
                assert_eq!(event.entity, None);
                level
            }
        };
        let pos = [event.pos.x, event.pos.y, event.pos.z];
        assert_eq!(level.block_state(pos), Some(event.state));
        assert_eq!(event.friction, 0.6);
        event.friction = level.friction(pos, event.state, event.entity).unwrap();
    }
}

struct Register;
#[mod_api::__subscriptions]
impl Hooks for Register {
    fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
        let key = r.block_key("test:friction_surface");
        REGISTERED_BLOCK.set(Some(key));
        r.block_part(
            key,
            WasmHandler {
                index: guest::add_handler(vec![guest::view::<dyn BlockPart>(Arc::new(Surface))]),
                module: WasmMod {
                    group: 0,
                    subscribed: &[],
                    listeners: &[],
                    corrupt: false,
                },
            },
        );
    }
}

#[test]
fn wire_loopback_friction_preserves_context_inputs_and_float_writeback() {
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new("test", Box::new(Register))].into();
    mod_api::testing::with_mods(mods.clone(), || {
        mod_api::__private::part_tests::install_parts_fixture(&mods).unwrap();
        mod_api::__private::resolve_block_keys(["test:friction_surface"], |_| {
            let props = std::collections::BTreeMap::new();
            sunrise_mod_runtime::block_definitions::BlockDefinition::new(&[&props], 0)
                .map_err(|e| format!("{e:?}"))
        })
        .unwrap();
        // Block parts dispatch from the resolved table the game installs at load.
        mod_api::__private::resolve_parts(
            &Default::default(),
            &Default::default(),
            mod_api::__private::KnownIds {
                block: &|_| true,
                item: &|_| true,
            },
        )
        .unwrap();
        let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
        let state = mod_api::BlockStateRecord::new(REGISTERED_BLOCK.get().unwrap(), 0).into();
        let world = World(state);
        let [x, y, z] = POS;
        for answer in [0.98, f32::NAN, f32::INFINITY, -0.5, 3.0] {
            ANSWER.set(answer);
            for (level, entity) in [
                (LevelSide::Server(&world), BOAT),
                (LevelSide::Client(&world), None),
            ] {
                let mut friction = 0.6;
                mod_api::dispatch_friction(
                    level,
                    mod_api::BlockPos::new(x, y, z),
                    state,
                    entity,
                    &mut friction,
                );
                assert_eq!(friction.to_bits(), answer.to_bits());
            }
        }
    });
}
