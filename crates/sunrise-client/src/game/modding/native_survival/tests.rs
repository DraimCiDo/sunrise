use super::{ACTIVE, Query, QueryGuard};
use crate::game::native_entity_refs::tests::{
    fixture_root, run_fresh_cases, with_client_game, with_game, with_game_at,
};
use crate::game::{Dim, Game, api_values::TryIntoApi};
use mod_api::__private::{ModPhase, PhaseOwner};
use mod_api::{
    BlockPart, BlockSide, BlockState, BlockStateRecord, BlockUpdateFlags, CanSurviveEvent,
    CanSustainPlantEvent, Hooks, HostTransaction, LevelReadError, LevelRef, LevelSide,
    MayPlaceOnEvent, Registrar, ServerHost, WorldEpoch,
};
use std::sync::{Arc, Mutex};
use sunrise_client::{chunk::Chunk, containers::BlockEntity, items::ItemStack};
use sunrise_core::position::BlockPos;

fn load_column(game: &mut Game<'_>) {
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
}

#[test]
#[ignore = "daily"]
fn parked_server_level_reads_its_own_support_and_container_slots() {
    struct Slots(Arc<Mutex<Option<LevelRef>>>);
    #[mod_api::sunrise_part]
    impl BlockPart for Slots {
        fn can_survive(&self, side: LevelSide<'_>, event: &mut CanSurviveEvent) -> Option<bool> {
            if event.pos.x != 7 {
                return None;
            }
            let LevelSide::Server(view) = side else {
                panic!("server uses server Parts")
            };
            let at = BlockSide {
                level: self.0.lock().unwrap().unwrap(),
                pos: [7, 64, 5],
                side: None,
            };
            Some(
                view.container_size(at) == Some(27)
                    && view
                        .container_stack(at, 0)
                        .is_some_and(|stack| stack.count() == 3),
            )
        }
    }
    #[mod_api::__subscriptions]
    impl Hooks for Slots {
        fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
            let torch = r.block_key("minecraft:torch");
            r.block_part(torch, Slots(Arc::clone(&self.0)));
        }
    }

    let level_ref = Arc::new(Mutex::new(None));
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
        "slot_survival",
        Box::new(Slots(Arc::clone(&level_ref))),
    )]
    .into();
    mod_platform::testing::with_mods(mods.clone(), || {
        crate::game::api_values::install_parts(&mods).unwrap();
        with_game(|game| {
            let _phase = PhaseOwner::scope(ModPhase::World);
            load_column(game);
            let registry = game.pack.assets.registry;
            let stone = registry.default_state("minecraft:stone").unwrap();
            let chest = registry.default_state("minecraft:chest").unwrap();
            let torch = registry
                .default_state("minecraft:torch")
                .unwrap()
                .try_into_api()
                .unwrap();
            game.world.set(5, 64, 5, stone).unwrap();
            // The candidate is not the block currently occupying the destination.
            game.world.set(5, 65, 5, stone).unwrap();
            game.world.set(7, 64, 5, chest).unwrap();
            let mut contents = vec![None; 27];
            contents[0] = Some(ItemStack::new("minecraft:diamond", 3));
            game.world
                .block_entities
                .insert((7, 64, 5), BlockEntity::Chest(contents));
            let overworld = game.level(game.dim.id()).unwrap();
            *level_ref.lock().unwrap() = Some(overworld);
            assert_eq!(overworld.can_survive(game, [7, 65, 5], torch), Ok(true));

            game.enter_level(Dim::Nether).unwrap();
            load_column(game);
            let nether = game.level(game.dim.id()).unwrap();
            assert_eq!(nether.can_survive(game, [5, 65, 5], torch), Ok(false));
            assert_eq!(overworld.can_survive(game, [5, 65, 5], torch), Ok(true));
            assert_eq!(nether.can_survive(game, [7, 65, 5], torch), Ok(false));
            assert_eq!(overworld.can_survive(game, [7, 65, 5], torch), Ok(true));
            assert_eq!(
                overworld.block_state(game, [5, 65, 5]),
                Some(stone.try_into_api().unwrap())
            );
            assert_eq!(game.dim, Dim::Nether);
        })
    });
}

#[test]
#[ignore = "daily"]
fn client_predicts_only_from_its_loaded_replica() {
    struct RainTorch;
    #[mod_api::sunrise_part]
    impl BlockPart for RainTorch {
        fn can_survive(&self, side: LevelSide<'_>, event: &mut CanSurviveEvent) -> Option<bool> {
            if event.pos.x != 6 {
                return None;
            }
            let LevelSide::Client(view) = side else {
                panic!("prediction uses client Parts")
            };
            Some(view.is_raining_at([event.pos.x, event.pos.y, event.pos.z]))
        }
    }
    #[mod_api::__subscriptions]
    impl Hooks for RainTorch {
        fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
            let torch = r.block_key("minecraft:torch");
            r.block_part(torch, RainTorch);
        }
    }

    let mods: mod_api::Mods =
        vec![mod_api::ModEntry::new("rain_torch", Box::new(RainTorch))].into();
    mod_platform::testing::with_mods(mods.clone(), || {
        crate::game::api_values::install_parts(&mods).unwrap();
        with_client_game(|game| {
            use crate::game::viewed_worlds::tests::open;
            let _phase = PhaseOwner::scope(ModPhase::World);
            let (client, _server) = sunrise_protocol::connection::Pipe::pair();
            game.connect_local(client, "SurvivalRead").unwrap();
            let remote = game.remote.as_mut().unwrap();
            remote.logged_in = true;
            let mut registries = sunrise_protocol::registry::Dynamic::default();
            registries.registries.insert(
                "minecraft:dimension_type".into(),
                vec![("minecraft:overworld".into(), None)],
            );
            remote.registries = Arc::new(registries);
            load_column(game);
            game.install_live_features();
            let registry = game.pack.assets.registry;
            let stone = registry.default_state("minecraft:stone").unwrap();
            let torch = registry
                .default_state("minecraft:torch")
                .unwrap()
                .try_into_api()
                .unwrap();
            game.world.set(5, 64, 5, stone).unwrap();
            let plains = game.res.biomes().id("minecraft:plains");
            let qy = (65 - game.world.bottom_y()) >> 2;
            Arc::make_mut(game.world.chunks.get_mut(&(0, 0)).unwrap()).set_biome_cell(
                6 >> 2,
                qy,
                5 >> 2,
                plains,
            );
            let level = game.native_client_level().unwrap();
            assert_eq!(level.can_survive(game, [5, 65, 5], torch), Ok(true));
            assert_eq!(level.can_survive(game, [6, 65, 5], torch), Ok(false));
            game.world.weather.rain_level = (1.0, 1.0);
            assert_eq!(level.can_survive(game, [6, 65, 5], torch), Ok(true));
            assert_eq!(
                level.can_survive(game, [16, 65, 5], torch),
                Err(LevelReadError::Unavailable)
            );

            let nether = "minecraft:the_nether";
            game.handle_packet(open(1, nether)).unwrap();
            game.remote
                .as_mut()
                .unwrap()
                .viewed
                .get_mut(1)
                .unwrap()
                .world
                .chunks
                .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
            let viewed = game.level(nether).unwrap();
            assert_eq!(
                viewed.can_survive(game, [5, 65, 5], torch),
                Err(LevelReadError::Unavailable)
            );

            game.remote.as_mut().unwrap().logged_in = false;
            assert_eq!(
                level.can_survive(game, [5, 65, 5], torch),
                Err(LevelReadError::Unavailable)
            );
        })
    });
}

#[test]
#[ignore = "daily"]
fn parts_settle_candidate_then_soil_then_plant_and_skip_disabled_foreign_parts() {
    struct Rules(Arc<Mutex<Vec<&'static str>>>);
    #[mod_api::sunrise_part]
    impl BlockPart for Rules {
        fn can_survive(&self, _: LevelSide<'_>, event: &mut CanSurviveEvent) -> Option<bool> {
            self.0.lock().unwrap().push("survival");
            match event.pos.x {
                1 => Some(false),
                2 => Some(true),
                _ => None,
            }
        }
        fn may_place_on(&self, _: LevelSide<'_>, _: &mut MayPlaceOnEvent) -> Option<bool> {
            self.0.lock().unwrap().push("plant");
            Some(true)
        }
    }

    struct Soil(Arc<Mutex<Vec<&'static str>>>);
    #[mod_api::sunrise_part]
    impl BlockPart for Soil {
        fn can_sustain_plant(
            &self,
            side: LevelSide<'_>,
            event: &mut CanSustainPlantEvent,
        ) -> Option<bool> {
            let LevelSide::Server(view) = side else {
                panic!("server uses server Parts")
            };
            let pos = event.ground_pos;
            assert_eq!(view.block_state([pos.x, pos.y, pos.z]), Some(event.ground));
            self.0.lock().unwrap().push("soil");
            match pos.x {
                3 => Some(false),
                4 => Some(true),
                _ => None,
            }
        }
    }

    #[mod_api::__subscriptions]
    impl Hooks for Rules {
        fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
            let plant = r.block_key("minecraft:dandelion");
            r.block_part(plant, Rules(Arc::clone(&self.0)));
            let ground = r.block_key("minecraft:stone");
            r.block_part(ground, Soil(Arc::clone(&self.0)));
        }
    }

    let calls = Arc::new(Mutex::new(Vec::new()));
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
        "survival_read",
        Box::new(Rules(Arc::clone(&calls))),
    )]
    .into();
    mod_platform::testing::with_mods(mods.clone(), || {
        crate::game::api_values::install_parts(&mods).unwrap();
        with_game(|game| {
            let _phase = PhaseOwner::scope(ModPhase::World);
            load_column(game);
            let registry = game.pack.assets.registry;
            let stone = registry.default_state("minecraft:stone").unwrap();
            let plant = registry.default_state("minecraft:dandelion").unwrap();
            let candidate = plant.try_into_api().unwrap();
            let level = game.level(game.dim.id()).unwrap();
            for (x, expected, expected_calls) in [
                (1, false, &["survival"][..]),
                (2, true, &["survival"][..]),
                (3, false, &["survival", "soil"][..]),
                (4, true, &["survival", "soil"][..]),
                (5, true, &["survival", "soil", "plant"][..]),
            ] {
                game.world.set(x, 64, 5, stone).unwrap();
                assert_eq!(level.can_survive(game, [x, 65, 5], candidate), Ok(expected));
                assert_eq!(std::mem::take(&mut *calls.lock().unwrap()), expected_calls);
            }
            // The native query must refuse an already active candidate before invoking the Part.
            {
                let _active = QueryGuard::enter(Query(
                    std::ptr::from_ref(&game.world),
                    BlockPos::new(5, 65, 5),
                    plant,
                ))
                .unwrap();
                assert_eq!(
                    level.can_survive(game, [5, 65, 5], candidate),
                    Err(LevelReadError::Unavailable)
                );
                assert!(calls.lock().unwrap().is_empty());
            }
            assert!(mod_api::__private::set_mod_enabled("survival_read", false));
            assert_eq!(level.can_survive(game, [5, 65, 5], candidate), Ok(false));
            game.world.set(5, 65, 5, plant).unwrap();
            assert_eq!(level.can_survive(game, [5, 65, 5], candidate), Ok(false));
            assert!(calls.lock().unwrap().is_empty());
        })
    });
}

#[test]
#[ignore = "daily"]
fn disabled_owned_part_retains_only_an_installed_block() {
    if run_fresh_cases(
        "game::native_survival::tests::disabled_owned_part_retains_only_an_installed_block",
        "SUNRISE_SURVIVAL_OWNED_CHILD",
        &["run"],
    ) {
        return;
    }
    const OWNED: &str = "survival_owner:fixture";
    struct Owned;
    #[mod_api::sunrise_part]
    impl BlockPart for Owned {
        fn can_survive(&self, _: LevelSide<'_>, _: &mut CanSurviveEvent) -> Option<bool> {
            Some(true)
        }
    }
    #[mod_api::__subscriptions]
    impl Hooks for Owned {
        fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
            let key = r.block_key(OWNED);
            r.block_part(key, Owned);
        }
        fn blocks(&self, _: &mod_api::RegistrationContext, event: &mut mod_api::BlocksEvent<'_>) {
            event
                .blocks
                .push(mod_api::Block::new(OWNED, "minecraft:glass"));
        }
    }

    let mods: mod_api::Mods =
        vec![mod_api::ModEntry::new("survival_owner", Box::new(Owned))].into();
    mod_platform::testing::with_mods(mods.clone(), || {
        crate::game::api_values::install_parts(&mods).unwrap();
        mod_api::__private::mark_game_thread();
        let root = crate::game::mods::content_root(&fixture_root(), &Default::default()).unwrap();
        with_game_at(root.prepared().to_path_buf(), |game| {
            let _phase = PhaseOwner::scope(ModPhase::World);
            load_column(game);
            let state = game.pack.assets.registry.default_state(OWNED).unwrap();
            let candidate = state.try_into_api().unwrap();
            let level = game.level(game.dim.id()).unwrap();
            assert_eq!(level.can_survive(game, [5, 65, 5], candidate), Ok(true));
            game.world.set(5, 65, 5, state).unwrap();
            assert!(mod_api::__private::set_mod_enabled("survival_owner", false));
            assert_eq!(level.can_survive(game, [5, 65, 5], candidate), Ok(true));
            assert_eq!(level.can_survive(game, [6, 65, 5], candidate), Ok(false));
        })
    });
}

#[test]
#[ignore = "daily"]
fn stale_invalid_unloaded_and_depth_refusals_are_not_survival_denials() {
    mod_platform::testing::with_mods(Default::default(), || {
        with_game(|game| {
            let _phase = PhaseOwner::scope(ModPhase::World);
            load_column(game);
            let torch: BlockState = game
                .pack
                .assets
                .registry
                .default_state("minecraft:torch")
                .unwrap()
                .try_into_api()
                .unwrap();
            let invalid = BlockStateRecord::new(torch.block(), u16::MAX).into();
            let level = game.level(game.dim.id()).unwrap();
            let at = [5, 65, 5];
            let stale = LevelRef::new(WorldEpoch::new(level.epoch().get() + 1), level.dimension());
            assert_eq!(
                stale.can_survive(game, at, torch),
                Err(LevelReadError::Stale)
            );
            assert_eq!(
                level.can_survive(game, at, invalid),
                Err(LevelReadError::Unavailable)
            );
            assert_eq!(
                level.can_survive(game, [16, 65, 5], torch),
                Err(LevelReadError::Unavailable)
            );
            assert_eq!(
                level.can_survive(game, [5, game.world.shape.max_y(), 5], torch),
                Err(LevelReadError::Unavailable)
            );
            game.world.chunks.insert(
                (30_000_000 >> 4, 0),
                Arc::new(Chunk::empty(30_000_000 >> 4, 0)),
            );
            assert_eq!(
                level.can_survive(game, [30_000_000, 65, 5], torch),
                Err(LevelReadError::Unavailable)
            );
            {
                let _setup = PhaseOwner::scope(ModPhase::Setup);
                assert_eq!(
                    level.can_survive(game, at, torch),
                    Err(LevelReadError::Unavailable)
                );
            }
            let state = game
                .pack
                .assets
                .registry
                .default_state("minecraft:torch")
                .unwrap();
            let limit = ACTIVE.with_borrow(|active| active.len());
            let nested = (0..limit)
                .map(|i| {
                    QueryGuard::enter(Query(
                        std::ptr::from_ref(&game.world),
                        BlockPos::new(i as i32, 70, 5),
                        state,
                    ))
                    .unwrap()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                level.can_survive(game, at, torch),
                Err(LevelReadError::Unavailable)
            );
            drop(nested);
            assert_eq!(level.can_survive(game, at, torch), Ok(false));
        })
    });
}

#[test]
#[ignore = "daily"]
fn survival_observes_tentative_support_without_writing_or_settling_the_transaction() {
    mod_platform::testing::with_mods(Default::default(), || {
        with_game(|game| {
            let _phase = PhaseOwner::scope(ModPhase::World);
            load_column(game);
            game.world.block_log = Some(Vec::new());
            let registry = game.pack.assets.registry;
            let state = |name| {
                registry
                    .default_state(name)
                    .unwrap()
                    .try_into_api()
                    .unwrap()
            };
            let (air, stone, torch): (BlockState, BlockState, BlockState) = (
                state("minecraft:air"),
                state("minecraft:stone"),
                state("minecraft:torch"),
            );
            let level = game.level(game.dim.id()).unwrap();
            let (support, target) = ([5, 64, 5], [5, 65, 5]);
            assert_eq!(level.can_survive(game, target, torch), Ok(false));
            let before = (
                game.block_ticks.blocks.count(),
                game.block_ticks.fluids.count(),
                game.level_events.len(),
                game.world.chunks.len(),
            );
            let host: &mut dyn ServerHost = game;
            let aborted = host.transaction(|host, tx| {
                assert_eq!(
                    level.set_block(host, support, stone, BlockUpdateFlags::NONE),
                    Ok(true)
                );
                assert_eq!(level.can_survive(host, target, torch), Ok(true));
                assert_eq!(level.can_survive(host, [6, 65, 5], torch), Ok(false));
                assert_eq!(
                    level.can_survive(host, [16, 65, 5], torch),
                    Err(LevelReadError::Unavailable)
                );
                assert_eq!(level.block_state(host, support), Some(stone));
                assert_eq!(level.block_state(host, target), Some(air));
                assert_eq!(host.transaction_current().as_ref(), Some(tx));
                Err::<(), _>("caller aborts")
            });
            assert_eq!(aborted, Err("caller aborts"));
            assert_eq!(level.block_state(game, support), Some(air));
            assert_eq!(level.block_state(game, target), Some(air));
            assert!(game.world.block_log.as_ref().unwrap().is_empty());
            assert_eq!(
                (
                    game.block_ticks.blocks.count(),
                    game.block_ticks.fluids.count(),
                    game.level_events.len(),
                    game.world.chunks.len()
                ),
                before
            );
        })
    });
}
