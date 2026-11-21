//! Observable host query semantics through real holder writes and transaction journals.
use super::*;
use crate::game::native_entity_refs::tests::with_game;
use mod_api::{
    AttachmentDescriptor, AttachmentKey, AttachmentQueries, AttachmentQueriesMut, HostTransaction,
};
use sunrise_client::chunk::Chunk;

struct QueryMod(Arc<Mutex<Option<AttachmentKey<u32>>>>);
#[mod_api::__subscriptions]
impl mod_api::Hooks for QueryMod {
    fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
        let key = r
            .register_attachment_type(AttachmentDescriptor::new("query_test:value", || 0u32))
            .unwrap();
        *self.0.lock().unwrap() = Some(key);
    }
}

fn with_query_game(test: impl FnOnce(&mut Game<'_>, &AttachmentKey<u32>)) {
    let key_slot = Arc::new(Mutex::new(None));
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
        "query_test",
        Box::new(QueryMod(key_slot.clone())),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            let key = key_slot
                .lock()
                .unwrap()
                .clone()
                .expect("QueryMod registered");
            test(game, &key);
        });
    });
}

fn values(game: &Game<'_>, key: &AttachmentKey<u32>) -> Vec<u32> {
    let host: &dyn mod_api::Host = game;
    host.query(key)
        .unwrap()
        .iter()
        .map(|row| *row.unwrap().value)
        .collect()
}

#[test]
#[ignore = "daily"]
fn query_snapshot_dimension_and_stale_writes() {
    with_query_game(|game, key| {
        game.options.dedicated = Some(Vec::new());
        game.native_level_opened(Dim::Overworld);
        assert!(values(game, key).is_empty());
        game.world
            .chunks
            .insert((2, 3), Arc::new(Chunk::empty(2, 3)));
        key.set(game, Holder::Chunk([2, 3]), 10).unwrap();
        let batch = game.query_mut(key).unwrap();
        let row = batch.iter().next().unwrap().unwrap();
        key.set(game, Holder::Chunk([2, 3]), 11).unwrap();
        assert_eq!(*row.value(), 10);
        assert_eq!(values(game, key), [11]);
        game.enter_level(Dim::Nether).unwrap();
        assert!(values(game, key).is_empty());
        assert_eq!(
            row.set(game, 12),
            Err(mod_api::AttachmentRowError::StaleHolder)
        );
        game.world
            .chunks
            .insert((2, 3), Arc::new(Chunk::empty(2, 3)));
        key.set(game, Holder::Chunk([2, 3]), 20).unwrap();
        assert_eq!(values(game, key), [20]);
        game.enter_level(Dim::Overworld).unwrap();
        assert_eq!(values(game, key), [11]);
        row.set(game, 12).unwrap();
        assert_eq!(values(game, key), [12]);
        let retained = game.world.chunks.remove(&(2, 3)).unwrap();
        assert!(values(game, key).is_empty());
        assert_eq!(*row.value(), 10);
        assert_eq!(
            row.remove(game),
            Err(mod_api::AttachmentRowError::StaleHolder)
        );
        game.world.chunks.insert((2, 3), retained);
        assert_eq!(
            row.set(game, 13),
            Err(mod_api::AttachmentRowError::StaleHolder)
        );
        let fresh = game.query_mut(key).unwrap();
        let fresh_row = fresh.iter().next().unwrap().unwrap();
        game.world
            .chunks
            .insert((2, 3), Arc::new(Chunk::empty(2, 3)));
        assert_eq!(
            fresh_row.set(game, 14),
            Err(mod_api::AttachmentRowError::StaleHolder)
        );
        assert!(values(game, key).is_empty());
    });
}

#[test]
#[ignore = "daily"]
fn query_writes_join_transaction_journal_without_implicit_commit() {
    with_query_game(|game, key| {
        game.options.dedicated = Some(Vec::new());
        game.native_level_opened(Dim::Overworld);
        game.world
            .chunks
            .insert((2, 3), Arc::new(Chunk::empty(2, 3)));
        key.set(game, Holder::Chunk([2, 3]), 10).unwrap();
        let batch = game.query_mut(key).unwrap();
        let row = batch.iter().next().unwrap().unwrap();
        let aborted: Result<(), ()> = game.transaction(|host, _| {
            row.set(host, 11).unwrap();
            assert_eq!(values(host, key), [11]);
            let current = host.query_mut(key).unwrap();
            current
                .iter()
                .next()
                .unwrap()
                .unwrap()
                .remove(host)
                .unwrap();
            assert!(values(host, key).is_empty());
            Err(())
        });
        assert_eq!(aborted, Err(()));
        assert_eq!(values(game, key), [10]);
        assert_eq!(*row.value(), 10);
        let committed: Result<(), ()> = game.transaction(|host, _| {
            let current = host.query_mut(key).unwrap();
            current
                .iter()
                .next()
                .unwrap()
                .unwrap()
                .set(host, 15)
                .unwrap();
            Ok(())
        });
        assert_eq!(committed, Ok(()));
        drop(batch);
        assert_eq!(values(game, key), [15]);
    });
}
