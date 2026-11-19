pub fn friction_game_thread() -> Arc<std::sync::atomic::AtomicUsize> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    struct Answer(f32, Arc<AtomicUsize>);
    #[mod_api::sunrise_part]
    impl BlockPart for Answer {
        fn friction(&self, _: crate::LevelSide<'_>, event: &mut FrictionEvent<'_>) {
            self.1.fetch_add(1, Ordering::SeqCst);
            event.friction = self.0;
        }
    }
    struct CopyMaterial(Arc<AtomicUsize>);
    #[mod_api::sunrise_part]
    impl BlockPart for CopyMaterial {
        fn friction(&self, side: crate::LevelSide<'_>, event: &mut FrictionEvent<'_>) {
            self.0.fetch_add(1, Ordering::SeqCst);
            if !matches!(event.entity, Some((_, "minecraft:player"))) {
                return;
            }
            let level = match side {
                crate::LevelSide::Server(level) | crate::LevelSide::Client(level) => level,
            };
            let pos = [event.pos.x, event.pos.y, event.pos.z];
            assert_eq!(level.block_state(pos), Some(event.state));
            // Asking the same state cannot dispatch this part again.
            assert_eq!(level.friction(pos, event.state, event.entity), None);
            // The neighbouring state stands in for a copycat's stored material. Ask
            // for that state at the copycat's position, not at the neighbour.
            if let Some(material) = level.block_state([pos[0], pos[1], pos[2] + 1])
                && let Some(friction) = level.friction(pos, material, event.entity)
            {
                event.friction = friction;
            }
        }
    }
    // Asks the same state for the next entity id until the engine refuses a nested query.
    struct Chain(Arc<AtomicUsize>);
    #[mod_api::sunrise_part]
    impl BlockPart for Chain {
        fn friction(&self, side: crate::LevelSide<'_>, event: &mut FrictionEvent<'_>) {
            self.0.fetch_add(1, Ordering::SeqCst);
            let Some((id, kind)) = event.entity else {
                return;
            };
            let level = match side {
                crate::LevelSide::Server(level) | crate::LevelSide::Client(level) => level,
            };
            let pos = [event.pos.x, event.pos.y, event.pos.z];
            event.friction = level
                .friction(pos, event.state, Some((id + 1, kind)))
                .unwrap_or(0.5);
        }
    }
    struct FrictionMod(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for FrictionMod {
        fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
            let copycat = r.block_key("minecraft:glass");
            r.block_part(copycat, CopyMaterial(self.0.clone()));
            let chain = r.block_key("minecraft:lime_wool");
            r.block_part(chain, Chain(self.0.clone()));
            for (block, answer) in [
                ("minecraft:white_wool", 0.98),
                ("minecraft:red_wool", f32::NAN),
                ("minecraft:blue_wool", 3.0),
                ("minecraft:green_wool", f32::INFINITY),
                ("minecraft:black_wool", -0.5),
            ] {
                let key = r.block_key(block);
                r.block_part(key, Answer(answer, self.0.clone()));
            }
        }
    }
    let mods: crate::Mods = vec![ModEntry::new(
        "friction_test",
        Box::new(FrictionMod(calls.clone())),
    )]
    .into();
    crate::__private::install(mods.clone());
    install_parts(&mods, &crate::parts::tests::rule_schema(), |_| false).unwrap();
    mark_game_thread();
    calls
}
