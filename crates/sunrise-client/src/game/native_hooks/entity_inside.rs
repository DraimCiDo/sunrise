use super::*;

/// The hook points. Each does nothing in Sunrise.
impl HitLevel<'_> {
    /// `Block.entityInside`: an entity's sweep crossed a block with a part for it
    /// (`precise` is `Entity.checkInsideBlocks`' flag), told mid-sweep, on the level
    /// host a hit or a landing uses. The part draws the level's random in vanilla's
    /// order and queues effects and owned callbacks on `event.apply`, `event.run_before`,
    /// and `event.run_after`, which flush at the end of the sweep via
    /// `InsideEffects::apply_and_clear` with live entity and level access.
    pub(in crate::game) fn native_entity_inside(
        &mut self,
        call: super::fire::EntityInsideCall<'_>,
    ) {
        self.fire_entity_inside(call);
    }
}

#[cfg(test)]
mod entity_inside_tests {
    use super::*;

    /// Landing writes reach the level at the batch end, in the order asked, and only
    /// where the block is still the state landed on.
    #[test]
    #[ignore = "daily"]
    fn landing_writes_apply_at_the_batch_end() {
        use crate::game::api_values::{TryIntoApi, TryIntoEngine};
        use sunrise_client::block_contact::{landed, take_pending};
        use sunrise_world::level::LandingWrite;
        crate::game::native_entity_refs::tests::with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            let state = |name| game.pack.assets.registry.default_state(name).unwrap();
            let (farmland, dirt, stone, air) = (
                state("minecraft:farmland"),
                state("minecraft:dirt"),
                state("minecraft:stone"),
                state("minecraft:air"),
            );
            game.world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
            );
            game.world.set(1, 64, 1, farmland).unwrap();
            game.world.set(2, 64, 1, farmland).unwrap();
            game.world.set(3, 64, 1, stone).unwrap();
            game.world.set(4, 64, 1, dirt).unwrap();
            game.world.set(5, 64, 1, dirt).unwrap();
            let drops_before = game.dropped.len();
            let at = |x| BlockPos::new(x, 64, 1);
            let requested = vec![
                mod_api::LandingWrite::SetBlock(stone.try_into_api().unwrap()),
                mod_api::LandingWrite::SetBlock(dirt.try_into_api().unwrap()),
            ];
            // Exercise the same complete-batch boundary as generated dispatch before
            // entering the engine queue, then prove its ordered writes reach the world.
            let staged = requested.try_into_engine(&*game).unwrap();
            landed(farmland, at(1), staged);
            landed(dirt, at(4), vec![LandingWrite::DestroyBlock { drop: true }]);
            landed(
                dirt,
                at(5),
                vec![LandingWrite::DestroyBlock { drop: false }],
            );
            // The block changed after the landing: dropped.
            landed(stone, at(2), vec![LandingWrite::SetBlock(dirt)]);
            landed(
                stone,
                at(3),
                vec![LandingWrite::DestroyBlock { drop: false }],
            );
            // Nothing asked: nothing kept.
            landed(farmland, at(1), Vec::new());
            assert_eq!(
                game.world.block(1, 64, 1),
                farmland,
                "nothing writes mid-batch"
            );
            game.world.block_log = Some(Vec::new());
            game.drain_entity_inside();
            assert_eq!(game.world.block(1, 64, 1), dirt);
            assert_eq!(game.world.block(2, 64, 1), farmland);
            assert_eq!(game.world.block(3, 64, 1), air, "the stone is destroyed");
            assert_eq!(game.world.block(4, 64, 1), air);
            assert_eq!(game.world.block(5, 64, 1), air);
            assert_eq!(
                game.dropped.len(),
                drops_before + 1,
                "only drop=true creates dirt resources"
            );
            let dropped = &game.dropped[drops_before];
            assert_eq!(dropped.stack.item.as_ref(), "minecraft:dirt");
            assert_eq!(
                dropped.position.x.floor() as i32,
                4,
                "resources came from drop=true, not the neighboring drop=false block"
            );
            // Both writes reach the clients (`setBlockAndUpdate` and `destroyBlock` are
            // flag 3); the dropped one sends nothing.
            let log = game.world.block_log.take().unwrap();
            assert!(log.contains(&(1, 64, 1)), "the set block is sent: {log:?}");
            assert!(
                log.contains(&(3, 64, 1)),
                "the destroyed block is sent: {log:?}"
            );
            assert!(
                !log.contains(&(2, 64, 1)),
                "the dropped write sends nothing"
            );
            assert!(take_pending().is_empty());
        });
    }
}

#[cfg(test)]
mod sweep_tests {
    use super::*;

    /// The `entity_inside` part the player's sweep tests install: it draws the
    /// level's random once a contact (vanilla's order), queues a push and a wither
    /// effect around the freeze, and can kill or panic on later contacts.
    #[derive(Clone, Default)]
    struct InsidePart {
        contacts: std::sync::Arc<std::sync::Mutex<Vec<[i32; 3]>>>,
        applied_order: std::sync::Arc<std::sync::Mutex<Vec<(&'static str, i32)>>>,
        wither: std::sync::Arc<std::sync::Mutex<Option<mod_api::EffectKey>>>,
        speed: std::sync::Arc<std::sync::Mutex<Option<mod_api::EffectKey>>>,
        kill_after: Option<usize>,
        panic_after: Option<usize>,
    }
    #[mod_api::sunrise_part]
    impl mod_api::BlockPart for InsidePart {
        fn entity_inside(
            &self,
            host: &mut dyn mod_api::LevelRandomHost,
            event: &mut mod_api::EntityInsideEvent<'_>,
        ) {
            let mut contacts = self.contacts.lock().unwrap();
            contacts.push([event.pos.x, event.pos.y, event.pos.z]);
            let seen = contacts.len();
            drop(contacts);
            // A level draw, in vanilla's order: between the vanilla contacts.
            let _ = host.with_random(|random| random.next_long());
            if self.panic_after.is_some_and(|after| seen > after) {
                panic!("an entity_inside part panics");
            }
            if self.kill_after.is_some_and(|after| seen > after) {
                event.run_after(mod_api::InsideEffect::Freeze, |view| {
                    view.hurt("minecraft:generic", 1000.0);
                });
            }
            event.run_after(mod_api::InsideEffect::Freeze, |view| {
                view.push([0.5, 0.0, 0.0]);
            });
            if let Some(wither) = *self.wither.lock().unwrap() {
                let order = self.applied_order.clone();
                event.run_after(mod_api::InsideEffect::Freeze, move |view| {
                    if view.add_effect(wither, 40, 0) {
                        order.lock().unwrap().push(("minecraft:wither", 40));
                    }
                });
            }
            if let Some(speed) = *self.speed.lock().unwrap() {
                let order = self.applied_order.clone();
                event.run_after(mod_api::InsideEffect::Extinguish, move |view| {
                    if view.add_effect(speed, 20, 0) {
                        order.lock().unwrap().push(("minecraft:speed", 20));
                    }
                });
            }
        }
    }
    struct InsideMod(InsidePart);
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for InsideMod {
        fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
            *self.0.wither.lock().unwrap() = Some(r.effect_key("minecraft:wither"));
            *self.0.speed.lock().unwrap() = Some(r.effect_key("minecraft:speed"));
            let stone = r.block_key("minecraft:stone");
            r.block_part(stone, self.0.clone());
        }
    }

    /// The part's mods, installed with their parts as a game's open does, before
    /// `with_game`: the game that opens then publishes the block index itself (the
    /// #824 parts-resolution order).
    fn install_inside_mod(name: &'static str, part: &InsidePart) -> mod_api::Mods {
        let entry =
            |part: &InsidePart| mod_api::ModEntry::new(name, Box::new(InsideMod(part.clone())));
        crate::game::api_values::install_parts(&[entry(part)]).unwrap();
        vec![entry(part)].into()
    }

    /// The game's player sweeps its move through two stone blocks, over the game's
    /// own world and assets, through the production `BLOCK_CONTACT_HOOKS` fire.
    /// Returns the player's velocity x before and after, and whether it died.
    fn sweep_game_player(game: &mut Game<'_>) -> (f64, f64, bool) {
        game.options.dedicated = Some(Vec::new());
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        game.world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
        );
        for x in [1, 2] {
            game.world.set(x, 100, 0, stone).unwrap();
        }
        if game.player.is_none() {
            game.player = Some(
                sunrise_client::motion::PlayerMotion::new(
                    game.motion,
                    sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5),
                )
                .unwrap(),
            );
        }
        game.player
            .as_mut()
            .unwrap()
            .history
            .add(sunrise_world::entity_move::MovementRecord {
                from: sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5),
                to: sunrise_core::geometry::Vec3::new(2.5, 100.0, 0.5),
                original: None,
            });
        sweep_game_player_live(
            game,
            sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5),
            false,
        )
    }

    /// Runs the server-turn player inside-block sweep: collect (optionally after
    /// `server_stuck_in_blocks`), flush live against `&mut Game`, and finish freezing.
    fn sweep_game_player_live(
        game: &mut Game<'_>,
        from: sunrise_core::geometry::Vec3,
        with_stuck_pass: bool,
    ) -> (f64, f64, bool) {
        use sunrise_client::motion::MotionWorld;
        let before = game.player.as_ref().unwrap().velocity.x;
        let effects = {
            let crate::game::Game {
                world,
                pack,
                motion,
                health_resources,
                damage_hooks,
                loop_context,
                collision,
                player,
                ..
            } = game;
            let entities: [sunrise_core::geometry::Aabb; 0] = [];
            sunrise_client::block_contact::in_level(
                sunrise_client::parallel::TickThread::mint(),
                Some("minecraft:overworld"),
                || {
                    let mut w = MotionWorld {
                        side: sunrise_client::motion::dry_side(),
                        tag_items: None,
                        block_registries: Some(&pack.block_registries),
                        loop_context,
                        damage_hooks,
                        entities: &entities,
                        player: player.as_mut().unwrap(),
                        scene: world,
                        assets: &pack.assets,
                        collision_binding: collision,
                        resources: motion,
                        health: health_resources,
                    };
                    if with_stuck_pass {
                        w.server_stuck_in_blocks(from).unwrap();
                    }
                    w.collect_server_block_damage(from).unwrap()
                },
            )
        };
        game.flush_player_inside(effects);
        {
            let crate::game::Game {
                world,
                pack,
                motion,
                health_resources,
                damage_hooks,
                loop_context,
                collision,
                player,
                ..
            } = game;
            let entities: [sunrise_core::geometry::Aabb; 0] = [];
            let mut w = MotionWorld {
                side: sunrise_client::motion::dry_side(),
                tag_items: None,
                block_registries: Some(&pack.block_registries),
                loop_context,
                damage_hooks,
                entities: &entities,
                player: player.as_mut().unwrap(),
                scene: world,
                assets: &pack.assets,
                collision_binding: collision,
                resources: motion,
                health: health_resources,
            };
            w.finish_server_block_damage_freezing();
        }
        let player = game.player.as_ref().unwrap();
        (before, player.velocity.x, player.health.dead_or_dying())
    }

    /// A listening part is told mid-walk, in walk order, and its callbacks reach
    /// the live player in this tick: both pushes add, and the wither effect lands
    /// synchronously on `game.potions.effects`.
    #[test]
    #[ignore = "daily"]
    fn a_part_pushes_the_player_and_its_effect_lands() {
        let part = InsidePart::default();
        let mods = install_inside_mod("inside-push", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let (before, after, dead) = sweep_game_player(game);
                assert!(!dead);
                assert_eq!(
                    *part.contacts.lock().unwrap(),
                    vec![[1, 100, 0], [2, 100, 0]],
                    "both contacts, in walk order"
                );
                assert!(
                    (after - before - 1.0).abs() < 1e-9,
                    "both pushes reached the player: {after}"
                );
                let effect = game
                    .potions
                    .effects
                    .get("wither")
                    .expect("the wither effect applied synchronously to the player");
                assert_eq!(effect.duration, 40);
                assert!(sunrise_client::block_contact::take_pending().is_empty());
            });
        });
    }

    /// A part that kills the player with its first write: the walk's later contacts
    /// still happen, but the flush stops at the death, so the later writes and
    /// effects never apply.
    #[test]
    #[ignore = "daily"]
    fn a_killing_write_stops_the_flush() {
        let part = InsidePart {
            kill_after: Some(0),
            ..InsidePart::default()
        };
        let mods = install_inside_mod("inside-kill", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let (before, after, dead) = sweep_game_player(game);
                assert_eq!(
                    part.contacts.lock().unwrap().len(),
                    2,
                    "the walk itself reaches both contacts"
                );
                assert!(dead, "the killing write applied");
                assert!(
                    (after - before).abs() < 1e-9,
                    "the writes after the death never apply"
                );
            });
        });
    }

    /// A part that panics on its first contact: the panic is contained, the mod's
    /// `entity_inside` callback is disabled for the session with a loud notice, the
    /// walk and the player's tick go on, and no staged callbacks apply.
    #[test]
    #[ignore = "daily"]
    fn a_panicking_part_leaves_the_players_tick_restored() {
        let part = InsidePart {
            panic_after: Some(0),
            ..InsidePart::default()
        };
        let mods = install_inside_mod("inside-panic", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let _ = mod_api::__private::take_callback_panics();
                let (before, after, dead) = sweep_game_player(game);
                assert!(!dead);
                assert_eq!(
                    part.contacts.lock().unwrap().len(),
                    1,
                    "contact #1 panicked and disabled this mod's entity_inside callback for the session"
                );
                let notices = mod_api::__private::take_callback_panics();
                assert!(
                    notices.iter().any(|n| {
                        n.mod_id == "inside-panic"
                            && n.hook == "entity_inside"
                            && n.detail.contains("an entity_inside part panics")
                    }),
                    "callback panic notice naming the mod was queued: {notices:?}"
                );
                assert!(
                    (after - before).abs() < 1e-9,
                    "the panicking callback's staged writes never applied"
                );
                assert!(
                    game.player
                        .as_ref()
                        .unwrap()
                        .events
                        .iter()
                        .any(|event| matches!(
                            event,
                            sunrise_client::motion::MotionEvent::EnterBlock { .. }
                        )),
                    "the vanilla moments still ran"
                );
            });
        });
    }

    /// Without a listening part the walk runs its vanilla moments and leaves the
    /// player's writes and the pending queue alone: no part is installed, so the
    /// fire's gate answers false and its DamageLevel is never built.
    #[test]
    #[ignore = "daily"]
    fn without_a_part_the_walk_costs_no_mod_work() {
        // The production binding, as every sweep test sets it: nothing is installed
        // on this thread, so its gate answers false for every state and the fire's
        // DamageLevel is never built. (A dummy binding here would race the
        // process-wide `block_contact::HOOKS` OnceLock — first setter wins —
        // against the other tests' real one.)
        let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
        crate::game::native_entity_refs::tests::with_game(|game| {
            let (before, after, dead) = sweep_game_player(game);
            assert!(!dead);
            assert!((after - before).abs() < 1e-9, "no writes without a part");
            assert!(
                game.player
                    .as_ref()
                    .unwrap()
                    .events
                    .iter()
                    .any(|event| matches!(
                        event,
                        sunrise_client::motion::MotionEvent::EnterBlock { .. }
                    )),
                "the vanilla moments still ran"
            );
            assert!(sunrise_client::block_contact::take_pending().is_empty());
        });
    }

    /// A part's `add_effect` callbacks keep their collector position:
    /// one queued after FREEZE and one after EXTINGUISH run in `applyAndClear`'s order,
    /// applying synchronously to `game.potions.effects`.
    #[test]
    #[ignore = "daily"]
    fn an_added_effect_keeps_its_collector_position() {
        let part = InsidePart::default();
        let mods = install_inside_mod("inside-order", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let (_, _, dead) = sweep_game_player(game);
                assert!(!dead);
                assert_eq!(
                    *part.applied_order.lock().unwrap(),
                    vec![("minecraft:wither", 40), ("minecraft:speed", 20),],
                    "each contact's callbacks flush in InsideEffect order: the FREEZE slot before the EXTINGUISH slot (second identical add_effect returns false)"
                );
                assert_eq!(game.potions.effects.get("wither").unwrap().duration, 40);
                assert_eq!(game.potions.effects.get("speed").unwrap().duration, 20);
            });
        });
    }

    /// A server's player is told once a tick: the early `ServerStuck` pass keeps
    /// the moves for `collect_server_block_damage`, and only the damage pass tells the
    /// part — two blocks, two contacts, not four, as vanilla's `entityInside`
    /// runs once a move.
    #[test]
    #[ignore = "daily"]
    fn a_servers_player_is_told_once_a_tick() {
        let part = InsidePart::default();
        let mods = install_inside_mod("inside-once", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let stone = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap();
                game.world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
                );
                for x in [1, 2] {
                    game.world.set(x, 100, 0, stone).unwrap();
                }
                if game.player.is_none() {
                    game.player = Some(
                        sunrise_client::motion::PlayerMotion::new(
                            game.motion,
                            sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5),
                        )
                        .unwrap(),
                    );
                }
                game.player.as_mut().unwrap().history.add(
                    sunrise_world::entity_move::MovementRecord {
                        from: sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5),
                        to: sunrise_core::geometry::Vec3::new(2.5, 100.0, 0.5),
                        original: None,
                    },
                );
                // The server turn's own order (lan_actions/tick.rs): the stuck
                // pass before the landing is checked, then the damage pass over
                // the same moves.
                sweep_game_player_live(
                    game,
                    sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5),
                    true,
                );
                assert_eq!(
                    *part.contacts.lock().unwrap(),
                    vec![[1, 100, 0], [2, 100, 0]],
                    "one contact a block, the damage pass's, not the stuck pass's too"
                );
            });
        });
    }

    /// Without a part the two server passes leave the player as vanilla has it:
    /// the walk still reports its blocks and nothing is queued.
    #[test]
    #[ignore = "daily"]
    fn the_server_passes_without_a_part_keep_vanilla_behavior() {
        crate::game::native_entity_refs::tests::with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            let stone = game
                .pack
                .assets
                .registry
                .default_state("minecraft:stone")
                .unwrap();
            game.world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
            );
            for x in [1, 2] {
                game.world.set(x, 100, 0, stone).unwrap();
            }
            if game.player.is_none() {
                game.player = Some(
                    sunrise_client::motion::PlayerMotion::new(
                        game.motion,
                        sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5),
                    )
                    .unwrap(),
                );
            }
            game.player
                .as_mut()
                .unwrap()
                .history
                .add(sunrise_world::entity_move::MovementRecord {
                    from: sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5),
                    to: sunrise_core::geometry::Vec3::new(2.5, 100.0, 0.5),
                    original: None,
                });
            let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
            let velocity = game.player.as_ref().unwrap().velocity.x;
            sweep_game_player_live(
                game,
                sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5),
                true,
            );
            assert_eq!(
                game.player.as_ref().unwrap().velocity.x,
                velocity,
                "no writes without a part"
            );
            let entered = game
                .player
                .as_ref()
                .unwrap()
                .events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        sunrise_client::motion::MotionEvent::EnterBlock { .. }
                    )
                })
                .count();
            assert_eq!(entered, 2, "each block still reported, vanilla's moments");
            assert!(sunrise_client::block_contact::take_pending().is_empty());
        });
    }

    /// The item's batch sweep tests: the same part, the same assertions as the
    /// player's, over `Game::sweep_item_inside`.
    fn item_at_blocks(game: &mut Game<'_>) -> sunrise_client::entities::ItemEntity {
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        game.world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
        );
        for x in [1, 2] {
            game.world.set(x, 100, 0, stone).unwrap();
        }
        let mut item = sunrise_client::entities::ItemEntity::new(
            7,
            sunrise_client::items::ItemStack::new("minecraft:stone", 1),
            sunrise_core::geometry::Vec3::new(2.5, 100.0, 0.5),
            sunrise_core::geometry::Vec3::ZERO,
            0.0,
        );
        item.previous = sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5);
        item
    }

    /// A part's pushes reach the item in its own tick, one contact a block.
    #[test]
    #[ignore = "daily"]
    fn a_part_pushes_an_item_once_a_block() {
        let part = InsidePart::default();
        let mods = install_inside_mod("inside-item", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let mut item = item_at_blocks(game);
                let before = item.velocity.x;
                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || {
                        game.sweep_item_inside(&mut item);
                    },
                );
                assert_eq!(
                    *part.contacts.lock().unwrap(),
                    vec![[1, 100, 0], [2, 100, 0]],
                    "one contact a block, in walk order"
                );
                assert!(
                    (item.velocity.x - before - 1.0).abs() < 1e-9,
                    "both pushes reached the item"
                );
                assert!(sunrise_client::block_contact::take_pending().is_empty());
            });
        });
    }

    /// A killing write stops the item's flush: the walk still reaches both blocks,
    /// the writes after the destruction never apply.
    #[test]
    #[ignore = "daily"]
    fn a_killing_write_stops_the_items_flush() {
        let part = InsidePart {
            kill_after: Some(0),
            ..InsidePart::default()
        };
        let mods = install_inside_mod("inside-item-kill", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let mut item = item_at_blocks(game);
                let before = item.velocity.x;
                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || {
                        game.sweep_item_inside(&mut item);
                    },
                );
                assert_eq!(part.contacts.lock().unwrap().len(), 2);
                assert!(item.health <= 0, "the killing write applied");
                assert!(
                    (item.velocity.x - before).abs() < 1e-9,
                    "the writes after the destruction never apply"
                );
            });
        });
    }

    /// A panicking part inside the item batch: generated dispatch contains it and
    /// disables the mod's `entity_inside` callback for the session, the lent level
    /// goes back, and the lifted row the batch holds returns to the level's list
    /// (#2194's and #2213's carrier discipline).
    #[test]
    #[ignore = "daily"]
    fn a_panicking_part_leaves_the_item_batch_restored() {
        let part = InsidePart {
            panic_after: Some(0),
            ..InsidePart::default()
        };
        let mods = install_inside_mod("inside-item-panic", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let _ = mod_api::__private::take_callback_panics();
                let item = item_at_blocks(game);
                game.dropped.push(item);
                let rows = game.dropped.len();
                let mut items = game.dropped.lift();
                let before = items[0].velocity.x;
                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || {
                        game.sweep_item_inside(&mut items[0]);
                    },
                );
                game.put_back_dropped(items);
                assert_eq!(
                    part.contacts.lock().unwrap().len(),
                    1,
                    "contact #1 panicked and disabled this mod's entity_inside callback for the session"
                );
                let notices = mod_api::__private::take_callback_panics();
                assert!(
                    notices
                        .iter()
                        .any(|n| { n.mod_id == "inside-item-panic" && n.hook == "entity_inside" }),
                    "callback panic notice naming the mod was queued: {notices:?}"
                );
                assert_eq!(game.dropped.len(), rows, "the lifted row returned");
                let item = game.dropped.first().unwrap();
                assert!(
                    (item.velocity.x - before).abs() < 1e-9,
                    "the panicking callback's staged writes never applied"
                );
                // The level went back: its blocks read as they were.
                assert!(
                    game.world.block(1, 100, 0).0 != 0,
                    "the level reads as it was"
                );
                assert!(sunrise_client::block_contact::take_pending().is_empty());
            });
        });
    }

    /// Without a part the item's sweep costs one gate test: nothing is written,
    /// nothing queued, exactly what main's recorded route did with no listener.
    #[test]
    #[ignore = "daily"]
    fn an_item_without_a_part_is_left_alone() {
        crate::game::native_entity_refs::tests::with_game(|game| {
            let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
            let mut item = item_at_blocks(game);
            let before = (item.velocity.x, item.health, item.fire_ticks);
            sunrise_client::block_contact::in_level(
                sunrise_client::parallel::TickThread::mint(),
                Some("minecraft:overworld"),
                || {
                    game.sweep_item_inside(&mut item);
                },
            );
            assert_eq!(
                (item.velocity.x, item.health, item.fire_ticks),
                before,
                "no writes without a part"
            );
            assert!(sunrise_client::block_contact::take_pending().is_empty());
        });
    }

    /// The projectile's batch sweep: a part's push reaches it, one contact a
    /// block, and a removed projectile's flush stops.
    #[test]
    #[ignore = "daily"]
    fn a_part_pushes_a_projectile_and_removal_stops_the_flush() {
        let part = InsidePart::default();
        let mods = install_inside_mod("inside-shot", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let stone = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap();
                game.world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
                );
                for x in [1, 2] {
                    game.world.set(x, 100, 0, stone).unwrap();
                }
                let mut shot = sunrise_client::projectiles::Projectile::new(
                    sunrise_client::projectiles::ProjectileKind::Snowball,
                    sunrise_core::geometry::Vec3::new(2.5, 100.0, 0.5),
                    None,
                );
                shot.id = 9;
                shot.previous = sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5);
                let before = shot.velocity.x;
                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || {
                        game.sweep_projectile_inside(&mut shot);
                    },
                );
                assert_eq!(
                    *part.contacts.lock().unwrap(),
                    vec![[1, 100, 0], [2, 100, 0]],
                    "one contact a block, in walk order"
                );
                assert!(
                    (shot.velocity.x - before - 1.0).abs() < 1e-9,
                    "both pushes reached the projectile"
                );
                // A removed arrow's flush stops: nothing applies past its death.
                part.contacts.lock().unwrap().clear();
                let mut arrow = sunrise_client::projectiles::Projectile::new(
                    sunrise_client::projectiles::ProjectileKind::Arrow {
                        base_damage: 1.0,
                        critical: false,
                        effect: None,
                    },
                    sunrise_core::geometry::Vec3::new(2.5, 100.0, 0.5),
                    None,
                );
                arrow.id = 10;
                arrow.previous = sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5);
                arrow.velocity = sunrise_core::geometry::Vec3::ZERO;
                let arrow_state = &mut arrow.arrow;
                if let Some(a) = arrow_state.as_mut() {
                    a.removed = true;
                }
                let before = arrow.velocity.x;
                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || {
                        game.sweep_projectile_inside(&mut arrow);
                    },
                );
                assert!(
                    part.contacts.lock().unwrap().is_empty(),
                    "a removed projectile is not walked"
                );
                assert!((arrow.velocity.x - before).abs() < 1e-9);
            });
        });
    }

    /// Without a part the projectile's sweep costs one gate test and nothing else,
    /// exactly what main's recorded route did with no listener.
    #[test]
    #[ignore = "daily"]
    fn a_projectile_without_a_part_is_left_alone() {
        crate::game::native_entity_refs::tests::with_game(|game| {
            let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
            let stone = game
                .pack
                .assets
                .registry
                .default_state("minecraft:stone")
                .unwrap();
            game.world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
            );
            game.world.set(1, 100, 0, stone).unwrap();
            let mut shot = sunrise_client::projectiles::Projectile::new(
                sunrise_client::projectiles::ProjectileKind::Snowball,
                sunrise_core::geometry::Vec3::new(1.5, 100.0, 0.5),
                None,
            );
            shot.previous = sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5);
            let before = (shot.velocity.x, shot.fire_ticks, shot.fall_distance);
            sunrise_client::block_contact::in_level(
                sunrise_client::parallel::TickThread::mint(),
                Some("minecraft:overworld"),
                || {
                    game.sweep_projectile_inside(&mut shot);
                },
            );
            assert_eq!(
                (shot.velocity.x, shot.fire_ticks, shot.fall_distance),
                before,
                "no writes without a part"
            );
            assert!(sunrise_client::block_contact::take_pending().is_empty());
        });
    }

    /// An arrow's contacts, on the same batch sweep the other projectiles':
    /// a flying arrow meets its blocks in walk order, once each, and a stuck
    /// arrow keeps the contact of the block it sits in.
    #[test]
    #[ignore = "daily"]
    fn an_arrow_is_told_once_a_block_stuck_and_flying() {
        let part = InsidePart::default();
        let mods = install_inside_mod("inside-arrow", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = sunrise_client::block_contact::HOOKS.set(BLOCK_CONTACT_HOOKS);
                let stone = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap();
                game.world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
                );
                for x in [1, 2] {
                    game.world.set(x, 100, 0, stone).unwrap();
                }
                let flying = arrow_shot(9, (0.5, 100.0, 0.5), (2.5, 100.0, 0.5));
                let before = flying.velocity.x;
                let mut flying = flying;
                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || {
                        game.sweep_projectile_inside(&mut flying);
                    },
                );
                assert_eq!(
                    *part.contacts.lock().unwrap(),
                    vec![[1, 100, 0], [2, 100, 0]],
                    "the flying arrow meets its blocks in walk order, once each"
                );
                assert!(
                    (flying.velocity.x - before - 1.0).abs() < 1e-9,
                    "the part's pushes applied to the arrow"
                );
                part.contacts.lock().unwrap().clear();
                // A stuck arrow: no move, the block it sits in still tells the part.
                let mut stuck = arrow_shot(10, (1.5, 100.0, 0.5), (1.5, 100.0, 0.5));
                stuck.velocity = sunrise_core::geometry::Vec3::ZERO;
                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || {
                        game.sweep_projectile_inside(&mut stuck);
                    },
                );
                assert_eq!(
                    *part.contacts.lock().unwrap(),
                    vec![[1, 100, 0]],
                    "the stuck arrow keeps the contact of its block"
                );
                assert!(sunrise_client::block_contact::take_pending().is_empty());
            });
        });
    }

    /// A panicking part inside the arrow batch: generated dispatch contains it and
    /// disables the mod's `entity_inside` callback for the session, the lifted
    /// projectile row returns to the level's list exactly once, and the arrow keeps
    /// its state.
    #[test]
    #[ignore = "daily"]
    fn a_panicking_part_leaves_the_arrow_list_whole() {
        let part = InsidePart {
            panic_after: Some(0),
            ..InsidePart::default()
        };
        let mods = install_inside_mod("inside-arrow-panic", &part);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = sunrise_client::block_contact::HOOKS.set(BLOCK_CONTACT_HOOKS);
                let _ = mod_api::__private::take_callback_panics();
                let stone = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap();
                game.world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
                );
                for x in [1, 2] {
                    game.world.set(x, 100, 0, stone).unwrap();
                }
                // The arrow joins the level's own list: the batch's lifted row.
                let shot = arrow_shot(11, (0.5, 100.0, 0.5), (2.5, 100.0, 0.5));
                let velocity = shot.velocity.x;
                let id = shot.id;
                game.creatures.projectiles.push(shot);
                let rows = game.creatures.projectiles.len();
                let mut projectiles = game.creatures.projectiles.lift();
                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || {
                        projectiles.update(0, |p| game.sweep_projectile_inside(p));
                    },
                );
                game.creatures.projectiles.restore(
                    projectiles,
                    sunrise_client::join_seq::RestoreOrder::SpawnsFirst,
                );
                assert_eq!(
                    part.contacts.lock().unwrap().len(),
                    1,
                    "contact #1 panicked and disabled this mod's entity_inside callback for the session"
                );
                let notices = mod_api::__private::take_callback_panics();
                assert!(
                    notices
                        .iter()
                        .any(|n| { n.mod_id == "inside-arrow-panic" && n.hook == "entity_inside" }),
                    "callback panic notice naming the mod was queued: {notices:?}"
                );
                assert_eq!(
                    game.creatures.projectiles.len(),
                    rows,
                    "the lifted row returned, exactly once"
                );
                let arrow = game
                    .creatures
                    .projectiles
                    .iter()
                    .next()
                    .expect("the arrow is still in the list");
                assert_eq!(arrow.id, id, "the same arrow, not lost nor duplicated");
                assert!(
                    (arrow.velocity.x - velocity).abs() < 1e-9,
                    "the panicking callback's staged writes never applied"
                );
                assert!(sunrise_client::block_contact::take_pending().is_empty());
            });
        });
    }

    /// An arrow for the sweep tests: an arrow state, its move recorded.
    fn arrow_shot(
        id: u32,
        from: (f64, f64, f64),
        to: (f64, f64, f64),
    ) -> sunrise_client::projectiles::Projectile {
        let mut shot = sunrise_client::projectiles::Projectile::new(
            sunrise_client::projectiles::ProjectileKind::Arrow {
                base_damage: 1.0,
                critical: false,
                effect: None,
            },
            sunrise_core::geometry::Vec3::new(to.0, to.1, to.2),
            None,
        );
        shot.id = id;
        shot.previous = sunrise_core::geometry::Vec3::new(from.0, from.1, from.2);
        shot
    }

    /// Set up a single stone block at `(1, 100, 0)` and return an item sweeping strictly
    /// inside that cell.
    fn single_stone_item(game: &mut Game<'_>) -> sunrise_client::entities::ItemEntity {
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        game.world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
        );
        game.world.set(1, 100, 0, stone).unwrap();
        let mut item = sunrise_client::entities::ItemEntity::new(
            7,
            sunrise_client::items::ItemStack::new("minecraft:stone", 1),
            sunrise_core::geometry::Vec3::new(1.5, 100.0, 0.5),
            sunrise_core::geometry::Vec3::ZERO,
            0.0,
        );
        item.previous = sunrise_core::geometry::Vec3::new(1.2, 100.0, 0.5);
        item
    }

    /// Callback level writes apply directly against the live level even after the touched
    /// block's state has changed; an invalid `BlockState` returns `false` every time without
    /// latching or rolling back sibling writes, and `destroy_block(..., true)` drops resources.
    #[test]
    #[ignore = "daily"]
    fn callback_level_writes_apply_after_block_state_changes_and_do_not_rollback_siblings() {
        use crate::game::api_values::TryIntoApi;
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Copy)]
        struct WriteStates {
            dirt: mod_api::BlockState,
            cobble: mod_api::BlockState,
            planks: mod_api::BlockState,
            invalid: mod_api::BlockState,
        }
        #[derive(Clone, Default)]
        struct WriteResults {
            dirt_set: Option<bool>,
            read_after_dirt: Option<Option<mod_api::BlockState>>,
            invalid_1: Option<bool>,
            read_after_invalid_1: Option<Option<mod_api::BlockState>>,
            cobble_set: Option<bool>,
            neighbor_destroyed: Option<bool>,
            invalid_2: Option<bool>,
            planks_set: Option<bool>,
        }
        #[derive(Clone, Default)]
        struct LevelWritePart {
            states: Arc<Mutex<Option<WriteStates>>>,
            results: Arc<Mutex<WriteResults>>,
        }
        #[mod_api::sunrise_part]
        impl mod_api::BlockPart for LevelWritePart {
            fn entity_inside(
                &self,
                _: &mut dyn mod_api::LevelRandomHost,
                event: &mut mod_api::EntityInsideEvent<'_>,
            ) {
                let WriteStates {
                    dirt,
                    cobble,
                    planks,
                    invalid,
                } = self.states.lock().unwrap().unwrap();
                let pos = [event.pos.x, event.pos.y, event.pos.z];
                let neighbor = [event.pos.x + 1, event.pos.y, event.pos.z];
                let second = [event.pos.x + 2, event.pos.y, event.pos.z];

                let before_results = self.results.clone();
                event.run_before(mod_api::InsideEffect::Freeze, move |view| {
                    let ok = view.set_block(pos, dirt);
                    let seen = view.block_state(pos);
                    let mut out = before_results.lock().unwrap();
                    out.dirt_set = Some(ok);
                    out.read_after_dirt = Some(seen);
                });

                let after_results = self.results.clone();
                event.run_after(mod_api::InsideEffect::Freeze, move |view| {
                    let invalid_1 = view.set_block(pos, invalid);
                    let after_invalid_1 = view.block_state(pos);
                    let cobble_set = view.set_block(pos, cobble);
                    let neighbor_destroyed = view.destroy_block(neighbor, true);
                    let invalid_2 = view.set_block(second, invalid);
                    let planks_set = view.set_block(second, planks);
                    let mut out = after_results.lock().unwrap();
                    out.invalid_1 = Some(invalid_1);
                    out.read_after_invalid_1 = Some(after_invalid_1);
                    out.cobble_set = Some(cobble_set);
                    out.neighbor_destroyed = Some(neighbor_destroyed);
                    out.invalid_2 = Some(invalid_2);
                    out.planks_set = Some(planks_set);
                });
            }
        }
        struct LevelWriteMod(LevelWritePart);
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for LevelWriteMod {
            fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
                let stone = r.block_key("minecraft:stone");
                r.block_part(stone, self.0.clone());
            }
        }

        let part = LevelWritePart::default();
        let entry =
            || mod_api::ModEntry::new("inside-level-writes", Box::new(LevelWriteMod(part.clone())));
        crate::game::api_values::install_parts(&[entry()]).unwrap();
        let mods: mod_api::Mods = vec![entry()].into();
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let state = |name| game.pack.assets.registry.default_state(name).unwrap();
                let (dirt, cobblestone, oak_planks, air) = (
                    state("minecraft:dirt"),
                    state("minecraft:cobblestone"),
                    state("minecraft:oak_planks"),
                    state("minecraft:air"),
                );
                let mut item = single_stone_item(game);
                game.world.set(2, 100, 0, dirt).unwrap();

                let dirt_api = dirt.try_into_api().unwrap();
                let cobble_api = cobblestone.try_into_api().unwrap();
                let planks_api = oak_planks.try_into_api().unwrap();
                let invalid_api =
                    mod_api::__private::block_state_from_ordinal(dirt_api.block(), u16::MAX);
                *part.states.lock().unwrap() = Some(WriteStates {
                    dirt: dirt_api,
                    cobble: cobble_api,
                    planks: planks_api,
                    invalid: invalid_api,
                });

                let drops_before = game.dropped.len();
                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || {
                        game.sweep_item_inside(&mut item);
                    },
                );

                let out = part.results.lock().unwrap().clone();
                assert_eq!(out.dirt_set, Some(true));
                assert_eq!(out.read_after_dirt, Some(Some(dirt_api)));
                assert_eq!(out.invalid_1, Some(false));
                assert_eq!(
                    out.read_after_invalid_1,
                    Some(Some(dirt_api)),
                    "invalid set_block must not roll back prior dirt write"
                );
                assert_eq!(
                    out.cobble_set,
                    Some(true),
                    "set_block succeeds even after touched block changed from stone to dirt"
                );
                assert_eq!(out.neighbor_destroyed, Some(true));
                assert_eq!(
                    out.invalid_2,
                    Some(false),
                    "second invalid set_block still returns false without latching"
                );
                assert_eq!(
                    out.planks_set,
                    Some(true),
                    "valid write right after second refusal still succeeds"
                );
                assert_eq!(game.world.block(1, 100, 0), cobblestone);
                assert_eq!(game.world.block(2, 100, 0), air);
                assert_eq!(game.world.block(3, 100, 0), oak_planks);
                assert_eq!(
                    game.dropped.len(),
                    drops_before + 1,
                    "destroy_block with drop = true drops neighbor resources"
                );
                let dropped = &game.dropped[drops_before];
                assert_eq!(dropped.stack.item.as_ref(), "minecraft:dirt");
                assert_eq!(dropped.position.x.floor() as i32, 2);
            });
        });
    }

    /// Calling `hurt_with_source` with a stale `EntityRef` refuses that hurt call
    /// (returns `false` without applying damage) and leaves sibling `push` and
    /// `set_block` writes intact without rolling them back.
    #[test]
    #[ignore = "daily"]
    fn callback_hurt_with_stale_source_does_not_refuse_or_rollback() {
        use crate::game::api_values::TryIntoApi;
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct StaleSourcePart {
            dirt: Arc<Mutex<Option<mod_api::BlockState>>>,
            outcomes: Arc<Mutex<Option<[bool; 3]>>>,
        }
        #[mod_api::sunrise_part]
        impl mod_api::BlockPart for StaleSourcePart {
            fn entity_inside(
                &self,
                _: &mut dyn mod_api::LevelRandomHost,
                event: &mut mod_api::EntityInsideEvent<'_>,
            ) {
                let dirt = self.dirt.lock().unwrap().unwrap();
                let pos = [event.pos.x, event.pos.y, event.pos.z];
                let stale_ref =
                    mod_api::EntityRef::new(mod_api::WorldEpoch::new(u64::MAX), 999_999);
                let outcomes = self.outcomes.clone();
                event.run_after(mod_api::InsideEffect::Freeze, move |view| {
                    let hurt_ok = view.hurt_with_source(
                        "minecraft:in_fire",
                        1.0,
                        Some(stale_ref),
                        Some(stale_ref),
                    );
                    let push_ok = view.push([0.5, 0.0, 0.0]);
                    let block_ok = view.set_block(pos, dirt);
                    *outcomes.lock().unwrap() = Some([hurt_ok, push_ok, block_ok]);
                });
            }
        }
        struct StaleSourceMod(StaleSourcePart);
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for StaleSourceMod {
            fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
                let stone = r.block_key("minecraft:stone");
                r.block_part(stone, self.0.clone());
            }
        }

        let part = StaleSourcePart::default();
        let entry = || {
            mod_api::ModEntry::new(
                "inside-stale-source",
                Box::new(StaleSourceMod(part.clone())),
            )
        };
        crate::game::api_values::install_parts(&[entry()]).unwrap();
        let mods: mod_api::Mods = vec![entry()].into();
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let dirt = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:dirt")
                    .unwrap();
                let mut item = single_stone_item(game);
                *part.dirt.lock().unwrap() = Some(dirt.try_into_api().unwrap());
                let health_before = item.health;
                let velocity_before = item.velocity.x;

                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || {
                        game.sweep_item_inside(&mut item);
                    },
                );

                assert_eq!(*part.outcomes.lock().unwrap(), Some([false, true, true]));
                assert_eq!(
                    item.health, health_before,
                    "stale explicit source refuses that hurt call without applying damage"
                );
                assert!((item.velocity.x - velocity_before - 0.5).abs() < 1e-9);
                assert_eq!(game.world.block(1, 100, 0), dirt);
            });
        });
    }

    /// A panicking `run_after` callback disables only its owning mod for `entity_inside`,
    /// leaves other mods running on the first and subsequent sweeps, and queues an error
    /// toast naming the panicking mod.
    #[test]
    #[ignore = "daily"]
    fn panicking_mod_callback_disables_only_that_mod_and_queues_error_toast() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        #[derive(Clone, Default)]
        struct PanickingPart {
            part_calls: Arc<AtomicUsize>,
            callback_runs: Arc<AtomicUsize>,
        }
        #[mod_api::sunrise_part]
        impl mod_api::BlockPart for PanickingPart {
            fn entity_inside(
                &self,
                _: &mut dyn mod_api::LevelRandomHost,
                event: &mut mod_api::EntityInsideEvent<'_>,
            ) {
                self.part_calls.fetch_add(1, Ordering::SeqCst);
                let runs = self.callback_runs.clone();
                event.run_after(mod_api::InsideEffect::Freeze, move |_view| {
                    runs.fetch_add(1, Ordering::SeqCst);
                    panic!("mod_panics run_after boom");
                });
            }
        }
        struct PanickingMod(PanickingPart);
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for PanickingMod {
            fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
                let stone = r.block_key("minecraft:stone");
                r.block_part(stone, self.0.clone());
            }
        }

        #[derive(Clone, Default)]
        struct HealthyPart {
            part_calls: Arc<AtomicUsize>,
            callback_runs: Arc<AtomicUsize>,
        }
        #[mod_api::sunrise_part]
        impl mod_api::BlockPart for HealthyPart {
            fn entity_inside(
                &self,
                _: &mut dyn mod_api::LevelRandomHost,
                event: &mut mod_api::EntityInsideEvent<'_>,
            ) {
                self.part_calls.fetch_add(1, Ordering::SeqCst);
                let runs = self.callback_runs.clone();
                event.run_after(mod_api::InsideEffect::Freeze, move |view| {
                    runs.fetch_add(1, Ordering::SeqCst);
                    view.push([0.25, 0.0, 0.0]);
                });
            }
        }
        struct HealthyMod(HealthyPart);
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for HealthyMod {
            fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
                let stone = r.block_key("minecraft:stone");
                r.block_part(stone, self.0.clone());
            }
        }

        let panics_part = PanickingPart::default();
        let healthy_part = HealthyPart::default();
        let make_entries = || {
            vec![
                mod_api::ModEntry::new("mod_panics", Box::new(PanickingMod(panics_part.clone()))),
                mod_api::ModEntry::new("mod_healthy", Box::new(HealthyMod(healthy_part.clone()))),
            ]
        };
        crate::game::api_values::install_parts(&make_entries()).unwrap();
        let mods: mod_api::Mods = make_entries().into();
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let _ = mod_api::__private::take_callback_panics();

                let mut item1 = single_stone_item(game);
                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || {
                        game.sweep_item_inside(&mut item1);
                    },
                );
                assert_eq!(panics_part.part_calls.load(Ordering::SeqCst), 1);
                assert_eq!(panics_part.callback_runs.load(Ordering::SeqCst), 1);
                assert_eq!(healthy_part.part_calls.load(Ordering::SeqCst), 1);
                assert_eq!(healthy_part.callback_runs.load(Ordering::SeqCst), 1);
                assert!((item1.velocity.x - 0.25).abs() < 1e-9);

                let mut item2 = single_stone_item(game);
                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || {
                        game.sweep_item_inside(&mut item2);
                    },
                );
                assert_eq!(
                    panics_part.part_calls.load(Ordering::SeqCst),
                    1,
                    "mod_panics must be skipped on the second sweep"
                );
                assert_eq!(panics_part.callback_runs.load(Ordering::SeqCst), 1);
                assert_eq!(
                    healthy_part.part_calls.load(Ordering::SeqCst),
                    2,
                    "mod_healthy still runs on the second sweep"
                );
                assert_eq!(healthy_part.callback_runs.load(Ordering::SeqCst), 2);
                assert!((item2.velocity.x - 0.25).abs() < 1e-9);

                // Client window presents system toasts for mod callback panics.
                game.options.execution_role = ExecutionRole::Client;
                game.add_mod_panic_toasts();
                assert!(
                    mod_api::__private::take_callback_panics().is_empty(),
                    "add_mod_panic_toasts drained the callback panic queue"
                );
                assert!(
                    game.server_packs
                        .mod_panics
                        .iter()
                        .any(|toast| toast.title.plain().contains("mod_panics")),
                    "expected a SystemToast naming mod_panics, got: {:?}",
                    game.server_packs
                        .mod_panics
                        .iter()
                        .map(|toast| toast.title.plain())
                        .collect::<Vec<_>>()
                );
            });
        });
    }

    /// Live player flush supports `set_block`/`destroy_block` with accurate `bool` returns
    /// (same-state `false`, already-air `false`), live `block_state`, Peaceful `add_effect`,
    /// Creative `set_remaining_fire_ticks` clamping, and fire-immune `is_on_fire` predicate.
    #[test]
    #[ignore = "daily"]
    fn player_live_inside_flush_level_writes_peaceful_effect_and_fire_semantics() {
        use crate::game::api_values::TryIntoApi;
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct LivePlayerPart {
            dirt: Arc<Mutex<Option<mod_api::BlockState>>>,
            speed: Arc<Mutex<Option<mod_api::EffectKey>>>,
            results: Arc<Mutex<Option<(bool, bool, bool, bool, bool, bool, bool, i32)>>>,
        }
        #[mod_api::sunrise_part]
        impl mod_api::BlockPart for LivePlayerPart {
            fn entity_inside(
                &self,
                _: &mut dyn mod_api::LevelRandomHost,
                event: &mut mod_api::EntityInsideEvent<'_>,
            ) {
                let dirt = self.dirt.lock().unwrap().unwrap();
                let speed = self.speed.lock().unwrap().unwrap();
                let pos = [event.pos.x, event.pos.y, event.pos.z];
                let results = self.results.clone();
                event.run_after(mod_api::InsideEffect::Freeze, move |view| {
                    let set_changed = view.set_block(pos, dirt);
                    let set_same = view.set_block(pos, dirt);
                    let saw_dirt = view.block_state(pos) == Some(dirt);
                    let destroy_first = view.destroy_block(pos, false);
                    let destroy_air = view.destroy_block(pos, false);
                    let effect_ok = view.add_effect(speed, 60, 0);
                    view.set_remaining_fire_ticks(100);
                    let on_fire = view.is_on_fire();
                    let ticks = view.fire_ticks();
                    *results.lock().unwrap() = Some((
                        set_changed,
                        set_same,
                        saw_dirt,
                        destroy_first,
                        destroy_air,
                        effect_ok,
                        on_fire,
                        ticks,
                    ));
                });
            }
        }
        struct LivePlayerMod(LivePlayerPart);
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for LivePlayerMod {
            fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
                *self.0.speed.lock().unwrap() = Some(r.effect_key("minecraft:speed"));
                let stone = r.block_key("minecraft:stone");
                r.block_part(stone, self.0.clone());
            }
        }

        let part = LivePlayerPart::default();
        let entry =
            || mod_api::ModEntry::new("inside-live-player", Box::new(LivePlayerMod(part.clone())));
        crate::game::api_values::install_parts(&[entry()]).unwrap();
        let mods: mod_api::Mods = vec![entry()].into();
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                game.ui.difficulty = 0; // Peaceful
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let stone = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap();
                let dirt = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:dirt")
                    .unwrap();
                *part.dirt.lock().unwrap() = Some(dirt.try_into_api().unwrap());
                game.world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
                );
                game.world.set(1, 100, 0, stone).unwrap();
                let mut player = sunrise_client::motion::PlayerMotion::new(
                    game.motion,
                    sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5),
                )
                .unwrap();
                player.creative = true;
                player.mod_fire_immune = true;
                player
                    .history
                    .add(sunrise_world::entity_move::MovementRecord {
                        from: sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5),
                        to: sunrise_core::geometry::Vec3::new(1.5, 100.0, 0.5),
                        original: None,
                    });
                game.player = Some(player);

                sweep_game_player_live(
                    game,
                    sunrise_core::geometry::Vec3::new(0.5, 100.0, 0.5),
                    false,
                );

                assert_eq!(
                    *part.results.lock().unwrap(),
                    Some((true, false, true, true, false, true, false, 1)),
                    "set/destroy returns, live read, Peaceful speed effect, fire-immune is_on_fire, and creative fire clamp (1)"
                );
                assert_eq!(game.potions.effects.get("speed").unwrap().duration, 60);
            });
        });
    }

    /// Item float-health subtraction before truncation, bundle drop on fatal hurt, and
    /// projectile `mark_hurt` + `is_on_fire`.
    #[test]
    #[ignore = "daily"]
    fn item_float_hurt_bundle_spill_and_projectile_mark_hurt() {
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct HurtEntityPart {
            fatal_item: Arc<Mutex<bool>>,
            proj_outcomes: Arc<Mutex<Option<(bool, bool)>>>,
        }
        #[mod_api::sunrise_part]
        impl mod_api::BlockPart for HurtEntityPart {
            fn entity_inside(
                &self,
                _: &mut dyn mod_api::LevelRandomHost,
                event: &mut mod_api::EntityInsideEvent<'_>,
            ) {
                let fatal = *self.fatal_item.lock().unwrap();
                let proj_outcomes = self.proj_outcomes.clone();
                event.run_after(mod_api::InsideEffect::Freeze, move |view| {
                    if view.kind() == "minecraft:item" {
                        if fatal {
                            view.hurt("minecraft:in_fire", 10.0);
                        } else {
                            view.hurt("minecraft:in_fire", 0.4);
                            view.hurt("minecraft:in_fire", 0.6);
                        }
                    } else {
                        let on_fire = view.is_on_fire();
                        let hurt = view.hurt("minecraft:generic", 1.0);
                        *proj_outcomes.lock().unwrap() = Some((on_fire, hurt));
                    }
                });
            }
        }
        struct HurtEntityMod(HurtEntityPart);
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for HurtEntityMod {
            fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
                let stone = r.block_key("minecraft:stone");
                r.block_part(stone, self.0.clone());
            }
        }

        let part = HurtEntityPart::default();
        let entry =
            || mod_api::ModEntry::new("inside-item-proj", Box::new(HurtEntityMod(part.clone())));
        crate::game::api_values::install_parts(&[entry()]).unwrap();
        let mods: mod_api::Mods = vec![entry()].into();
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);

                // Fractional damage: 5.0 - 0.4 = 4.6 -> 4, then 4.0 - 0.6 = 3.4 -> 3.
                let mut item = single_stone_item(game);
                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || game.sweep_item_inside(&mut item),
                );
                assert_eq!(item.health, 3, "float health subtraction before truncation");

                // Fatal damage on a bundle spawns its contained items.
                *part.fatal_item.lock().unwrap() = true;
                let mut bundle = sunrise_client::items::ItemStack::new("minecraft:bundle", 1);
                bundle
                    .data_mut()
                    .bundle_contents
                    .push(sunrise_client::items::ItemStack::new("minecraft:stick", 2));
                let mut bundle_item = sunrise_client::entities::ItemEntity::new(
                    8,
                    bundle,
                    sunrise_core::geometry::Vec3::new(1.5, 100.0, 0.5),
                    sunrise_core::geometry::Vec3::ZERO,
                    0.0,
                );
                bundle_item.previous = sunrise_core::geometry::Vec3::new(1.2, 100.0, 0.5);
                let dropped_before = game.dropped.len();
                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || game.sweep_item_inside(&mut bundle_item),
                );
                assert!(bundle_item.health <= 0);
                assert!(
                    game.dropped.len() > dropped_before
                        && game
                            .dropped
                            .iter()
                            .any(|d| d.stack.item == "minecraft:stick" && d.stack.count == 2),
                    "fatal item hurt spills bundle contents"
                );

                // Projectile `hurt` sets `sync_velocity` (`markHurt`) and `is_on_fire` matches `shot.on_fire()`.
                let mut shot = arrow_shot(9, (1.2, 100.0, 0.5), (1.5, 100.0, 0.5));
                shot.fire_ticks = 40;
                sunrise_client::block_contact::in_level(
                    sunrise_client::parallel::TickThread::mint(),
                    Some("minecraft:overworld"),
                    || game.sweep_projectile_inside(&mut shot),
                );
                assert_eq!(*part.proj_outcomes.lock().unwrap(), Some((true, false)));
                assert!(
                    shot.sync_velocity,
                    "projectile hurt sets sync_velocity (markHurt)"
                );
            });
        });
    }

    /// Vanilla powder-snow melt and fire ignition/hurt queue into `InsideEffects` so mod
    /// callbacks order against them (`run_after(Freeze)` before `PowderSnowMelt` before
    /// `Extinguish`; `run_after(FireIgnite)` sees post-ignition `fire_ticks`).
    #[test]
    #[ignore = "daily"]
    fn player_powder_snow_and_fire_builtin_ordering() {
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct BuiltinOrderPart {
            snow_key: Arc<Mutex<Option<mod_api::BlockKey>>>,
            fire_key: Arc<Mutex<Option<mod_api::BlockKey>>>,
            kill_in_snow: Arc<Mutex<bool>>,
            fire_ticks_after_ignite: Arc<Mutex<Option<i32>>>,
        }
        #[mod_api::sunrise_part]
        impl mod_api::BlockPart for BuiltinOrderPart {
            fn entity_inside(
                &self,
                _: &mut dyn mod_api::LevelRandomHost,
                event: &mut mod_api::EntityInsideEvent<'_>,
            ) {
                let kill = *self.kill_in_snow.lock().unwrap();
                let saw_fire = self.fire_ticks_after_ignite.clone();
                if Some(event.state.block()) == *self.snow_key.lock().unwrap() {
                    event.run_after(mod_api::InsideEffect::Freeze, move |view| {
                        view.set_remaining_fire_ticks(40);
                        if kill {
                            view.hurt("minecraft:generic", 1000.0);
                        }
                    });
                } else if Some(event.state.block()) == *self.fire_key.lock().unwrap() {
                    event.run_after(mod_api::InsideEffect::FireIgnite, move |view| {
                        *saw_fire.lock().unwrap() = Some(view.fire_ticks());
                    });
                }
            }
        }
        struct BuiltinOrderMod(BuiltinOrderPart);
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for BuiltinOrderMod {
            fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
                let snow = r.block_key("minecraft:powder_snow");
                let fire = r.block_key("minecraft:fire");
                *self.0.snow_key.lock().unwrap() = Some(snow);
                *self.0.fire_key.lock().unwrap() = Some(fire);
                r.block_part(snow, self.0.clone());
                r.block_part(fire, self.0.clone());
            }
        }

        let part = BuiltinOrderPart::default();
        let entry = || {
            mod_api::ModEntry::new(
                "inside-builtin-order",
                Box::new(BuiltinOrderMod(part.clone())),
            )
        };
        crate::game::api_values::install_parts(&[entry()]).unwrap();
        let mods: mod_api::Mods = vec![entry()].into();
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                let _ = sunrise_client::block_contact::HOOKS.set(super::BLOCK_CONTACT_HOOKS);
                let snow = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:powder_snow")
                    .unwrap();
                let fire = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:fire")
                    .unwrap();
                let air = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:air")
                    .unwrap();
                game.world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
                );

                // 1) Igniting in `run_before(Extinguish)` melts powder snow before Extinguish runs.
                game.world.set(1, 100, 0, snow).unwrap();
                let mut player = sunrise_client::motion::PlayerMotion::new(
                    game.motion,
                    sunrise_core::geometry::Vec3::new(1.5, 100.0, 0.5),
                )
                .unwrap();
                player
                    .history
                    .add(sunrise_world::entity_move::MovementRecord {
                        from: sunrise_core::geometry::Vec3::new(1.2, 100.0, 0.5),
                        to: sunrise_core::geometry::Vec3::new(1.5, 100.0, 0.5),
                        original: None,
                    });
                game.player = Some(player);
                sweep_game_player_live(
                    game,
                    sunrise_core::geometry::Vec3::new(1.2, 100.0, 0.5),
                    false,
                );
                assert_eq!(
                    game.world.block(1, 100, 0),
                    air,
                    "igniting before Extinguish triggers vanilla powder-snow melt"
                );
                assert!(
                    !game.player.as_ref().unwrap().is_on_fire(),
                    "Extinguish then puts out the player"
                );

                // 2) Killing in `run_before(Extinguish)` stops the flush before powder-snow melt.
                *part.kill_in_snow.lock().unwrap() = true;
                game.world.set(1, 100, 0, snow).unwrap();
                let mut player = sunrise_client::motion::PlayerMotion::new(
                    game.motion,
                    sunrise_core::geometry::Vec3::new(1.5, 100.0, 0.5),
                )
                .unwrap();
                player
                    .history
                    .add(sunrise_world::entity_move::MovementRecord {
                        from: sunrise_core::geometry::Vec3::new(1.2, 100.0, 0.5),
                        to: sunrise_core::geometry::Vec3::new(1.5, 100.0, 0.5),
                        original: None,
                    });
                game.player = Some(player);
                sweep_game_player_live(
                    game,
                    sunrise_core::geometry::Vec3::new(1.2, 100.0, 0.5),
                    false,
                );
                assert_eq!(
                    game.world.block(1, 100, 0),
                    snow,
                    "death before PowderSnowMelt stops the flush before melting the snow"
                );

                // 3) Standing in fire: `run_after(FireIgnite)` sees post-ignition fire_ticks.
                game.world.set(1, 100, 0, fire).unwrap();
                let mut player = sunrise_client::motion::PlayerMotion::new(
                    game.motion,
                    sunrise_core::geometry::Vec3::new(1.5, 100.0, 0.5),
                )
                .unwrap();
                player.fire_ticks = 0;
                player
                    .history
                    .add(sunrise_world::entity_move::MovementRecord {
                        from: sunrise_core::geometry::Vec3::new(1.2, 100.0, 0.5),
                        to: sunrise_core::geometry::Vec3::new(1.5, 100.0, 0.5),
                        original: None,
                    });
                game.player = Some(player);
                sweep_game_player_live(
                    game,
                    sunrise_core::geometry::Vec3::new(1.2, 100.0, 0.5),
                    false,
                );
                assert!(
                    part.fire_ticks_after_ignite
                        .lock()
                        .unwrap()
                        .is_some_and(|t| t > 0),
                    "run_after(FireIgnite) observes post-ignition fire_ticks"
                );
            });
        });
    }
}
