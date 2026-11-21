//! Read-only equipment host: prepared facts for the target, ordinary game reads elsewhere.
use super::Game;
use mod_api::__private::game::BackendRefOf;
use mod_api::{
    Backend, BackendRef, EntityRef, EntityTypeKey, EntityView, EquipmentSlot, Host, ItemStack,
};

/// One target's read view, captured before slot traversal and compared again before apply.
// no wire: borrowed by the native equipment host and never sent out of process.
#[derive(PartialEq)]
pub(in crate::game) struct EquipSnapshot {
    pub(in crate::game) entity: EntityRef,
    pub(in crate::game) kind: Option<EntityTypeKey>,
    pub(in crate::game) position: Option<[f64; 3]>,
    pub(in crate::game) health: Option<f32>,
    pub(in crate::game) max_health: Option<f32>,
    pub(in crate::game) alive: Option<bool>,
    pub(in crate::game) living: bool,
    // Slots in EquipmentSlot order; empty or unsupported slots are None.
    pub(in crate::game) equipment: [Option<ItemStack>; 8],
}

// no wire: borrows the equipment decision's prepared and live reads.
struct EquipEntityView<'a> {
    row: &'a EquipSnapshot,
    live: Option<&'a dyn EntityView>,
}

impl EntityView for EquipEntityView<'_> {
    fn position(&self) -> Option<[f64; 3]> {
        self.row.position
    }

    fn block_position(&self) -> Option<[i32; 3]> {
        self.live?.block_position()
    }

    fn rotation(&self) -> Option<[f32; 2]> {
        self.live?.rotation()
    }

    fn kind(&self) -> Option<EntityTypeKey> {
        self.row.kind
    }
}

pub(in crate::game) struct EquipHost<'g, 'a> {
    pub(in crate::game) game: &'g Game<'a>,
    pub(in crate::game) row: &'g EquipSnapshot,
}
impl EquipHost<'_, '_> {
    pub(in crate::game) fn with_entity_view(
        &self,
        entity: EntityRef,
        read: &mut dyn FnMut(&dyn EntityView),
    ) {
        if entity == self.row.entity {
            // The decision's prepared position and type stay fixed. The fields not
            // prepared for the decision still come from the ordinary game view.
            let live = self.game.resolve_entity_view(entity);
            read(&EquipEntityView {
                row: self.row,
                live: live.as_ref().map(|view| view as &dyn EntityView),
            });
        } else {
            self.game.with_entity_view(entity, read);
        }
    }

    pub(in crate::game) fn equipment(&self, slot: EquipmentSlot) -> Option<ItemStack> {
        let index = match slot {
            EquipmentSlot::MainHand => 0,
            EquipmentSlot::OffHand => 1,
            EquipmentSlot::Feet => 2,
            EquipmentSlot::Legs => 3,
            EquipmentSlot::Chest => 4,
            EquipmentSlot::Head => 5,
            EquipmentSlot::Body => 6,
            EquipmentSlot::Saddle => 7,
        };
        self.row.equipment[index].clone()
    }
}
impl Host for EquipHost<'_, '_> {
    fn backend(&self) -> Option<BackendRef<'_>> {
        Some(BackendRef::of(self))
    }
}
impl Backend for EquipHost<'_, '_> {}
