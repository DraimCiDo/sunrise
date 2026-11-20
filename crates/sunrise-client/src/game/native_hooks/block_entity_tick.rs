use super::*;
use crate::game::{api_values::TryIntoApi, transactions::OpenTransactions};
use mod_api::{
    BlockEntityTickData, BlockEntityTickSubject, BlockEntityView, HookId, ModBlockEntity,
};
use std::cell::Cell;

thread_local! {
    // Only the fixed-work comparison selects the existing handle API at this same tick site.
    static HANDLE_BENCHMARK: Cell<bool> = const { Cell::new(false) };
}

struct TickData<'a> {
    entity: &'a mut EngineBlockEntity,
    journal: &'a mut OpenTransactions,
}

impl BlockEntityTickData for TickData<'_> {
    fn data_slots(&self) -> &[i32] {
        &self.entity.data
    }

    fn set_data_slot(&mut self, slot: u32, value: i32) -> bool {
        let index = slot as usize;
        if self.entity.data.get(index).is_none_or(|old| *old == value) {
            return false;
        }
        self.journal.note_block_entity_write();
        self.entity.data[index] = value;
        true
    }
}

impl Game<'_> {
    pub(in crate::game) fn benchmark_block_entity_tick_handle(enabled: bool) {
        HANDLE_BENCHMARK.set(enabled);
    }

    pub(in crate::game) fn native_block_entity_tick_view(
        kind: mod_api::BlockEntityTypeKey,
        subject: mod_api::BlockEntityTickSubject<'_>,
    ) {
        Self::fire_block_entity_tick_view(kind, subject);
    }
    /// Tick the existing individual block entity moment. Unchosen replies use full write-back.
    pub(in crate::game) fn native_block_entity_tick(
        &mut self,
        reply: &mut mod_api::BlockEntityTickReply,
        pos: [i32; 3],
        entity: &mut ModBlockEntity,
    ) {
        self.fire_block_entity_tick(reply, pos, entity);
    }

    /// The tick of the mod block entity at `pos`.
    pub(in crate::game) fn tick_mod_block_entity_loaded(&mut self, pos: Pos) {
        if self.try_tick_resident_block_entity(pos) {
            return;
        }
        let Some(BlockEntity::Mod(m)) = self.world.block_entities.get_mut(&pos) else {
            return;
        };
        let mut taken = std::mem::take(&mut **m);
        let placeholder_identity = m.revision().identity();
        let state = self.world.block(pos.0, pos.1, pos.2).0;
        let slots = self
            .world
            .block_entities
            .mod_container(state)
            .map_or(0, |c| c.slots);
        let mut items = taken.items.clone();
        if items.len() < slots {
            items.resize(slots, None);
        }
        let parent = (
            self.block_entity_tick_key
                .replace(crate::game::block_entity_tick_items::Key {
                    pos,
                    placeholder_identity,
                    transaction_depth: self.transactions.len(),
                    unbalanced: false,
                }),
            self.block_entity_tick_items.take(),
        );
        let changed = self.api_block_entity_tick([pos.0, pos.1, pos.2], &mut taken, slots);
        self.finish_tick_item_writeback(pos, taken, &items, parent, changed);
    }

    /// Keep the existing snapshot path for types that do not opt into the prototype.
    fn try_tick_resident_block_entity(&mut self, pos: Pos) -> bool {
        let hook = HookId::BlockEntityTickView;
        if !mod_api::__private::has_part_hook(hook) || !self.is_logical_server() {
            return false;
        }
        let Some(BlockEntity::Mod(entity)) = self.world.block_entities.get(&pos) else {
            return false;
        };
        if !mod_api::__private::block_entity_type_listens(hook, entity.id()) {
            return false;
        }
        let Some(kind) = mod_api::__private::block_entity_type_key(entity.id()) else {
            return false;
        };
        let (x, y, z) = pos;
        let Some(level) = self.level(self.dim.id()) else {
            return false;
        };
        let Some(generation) = self.world.block_entities.generation(&pos) else {
            return false;
        };
        let handle = mod_api::__private::game::block_entity_ref(level, [x, y, z], generation);
        if HANDLE_BENCHMARK.get() {
            Self::native_block_entity_tick_view(
                kind,
                BlockEntityTickSubject::Handle {
                    host: self,
                    entity: handle,
                },
            );
            return true;
        }
        let Ok(state) = self.world.block(x, y, z).try_into_api() else {
            return false;
        };
        self.keep_block_entity(pos);
        // The journal does not call listeners or replace the resolved subject.
        let Some(BlockEntity::Mod(entity)) = self.world.block_entities.get_mut(&pos) else {
            unreachable!("the resident block entity changed while preparing its journal");
        };
        let mut data = TickData {
            entity,
            journal: &mut self.transactions,
        };
        let view = BlockEntityView::new(handle, state, &mut data);
        Self::native_block_entity_tick_view(kind, BlockEntityTickSubject::View(view));
        true
    }

    /// Restore only this tick's original placeholder, with no tick-only effects.
    pub(in crate::game) fn restore_unchanged_tick(&mut self, pos: Pos, taken: EngineBlockEntity) {
        if let Some(BlockEntity::Mod(entity)) = self.world.block_entities.get_mut(&pos)
            && self.block_entity_tick_key.is_some_and(|key| {
                key.pos == pos
                    && entity.id().is_empty()
                    && entity.revision().identity() == key.placeholder_identity
            })
        {
            **entity = taken;
        } else {
            self.drop_unreturned_mod_contents(pos, taken);
        }
    }
}
