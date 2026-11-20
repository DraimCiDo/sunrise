//! Scoped access to a resident server block entity's menu data.
use crate::{BlockEntityRef, BlockState, LevelRef, ServerHost};

// no wire: a call-scoped loan of live data, not a payload.
/// The block entity this server ticker is running. It stays in the level during
/// the call. Keep [`Self::handle`] rather than keeping the view.
///
/// ```compile_fail,E0521
/// use mod_api::BlockEntityView;
/// fn save(view: BlockEntityView<'_>, later: &mut Option<BlockEntityView<'static>>) {
///     *later = Some(view);
/// }
/// ```
// exhaustive: private fields keep construction on the engine side.
pub struct BlockEntityView<'call> {
    handle: BlockEntityRef,
    state: BlockState,
    data: &'call mut dyn BlockEntityTickData,
}

impl<'call> BlockEntityView<'call> {
    /// Lend the already resolved subject and checked data editor. Engine only.
    #[doc(hidden)]
    pub fn new(
        handle: BlockEntityRef,
        state: BlockState,
        data: &'call mut dyn BlockEntityTickData,
    ) -> Self {
        Self {
            handle,
            state,
            data,
        }
    }

    /// The entity's block position.
    pub fn position(&self) -> [i32; 3] {
        self.handle.position
    }

    /// The entity's block state at entry to the tick.
    pub fn state(&self) -> BlockState {
        self.state
    }

    /// The level the entity is ticking in.
    pub fn level(&self) -> LevelRef {
        self.handle.level
    }

    /// A generation-bound handle that may be retained after the call.
    pub fn handle(&self) -> BlockEntityRef {
        self.handle
    }

    /// The live typed integer slots the entity's menu reads.
    pub fn data_slots(&self) -> &[i32] {
        self.data.data_slots()
    }

    /// Set one existing slot. Returns false for an absent slot or an equal
    /// value. The write joins the ambient transaction if one is open; it does
    /// not send a container notification, like the matching handle setter.
    pub fn set_data_slot(&mut self, slot: u32, value: i32) -> bool {
        self.data.set_data_slot(slot, value)
    }
}

/// Game-side backing data for [`BlockEntityView`].
#[doc(hidden)]
pub trait BlockEntityTickData {
    /// Read through to the resolved slots.
    fn data_slots(&self) -> &[i32];
    /// Check one slot, then write it under the engine's journal.
    fn set_data_slot(&mut self, slot: u32, value: i32) -> bool;
}

// no wire: benchmark modes lend an exclusive tick context.
/// Branch-only A/B route for the resident ticker prototype.
#[non_exhaustive]
pub enum BlockEntityTickSubject<'call> {
    /// One live, call-scoped entity.
    View(BlockEntityView<'call>),
    /// The same resident subject through the existing handle API.
    #[doc(hidden)]
    Handle {
        /// The existing, mutable server API.
        host: &'call mut dyn ServerHost,
        /// The entity ticking in this call.
        entity: BlockEntityRef,
    },
}
