//! Typed state a block entity Part keeps on each block entity of its type (#781, #796).
//!
//! The value lives in the block entity's own attachment map, the store every other
//! attachment uses: it saves with the chunk through the type's codec, is kept by
//! transactions, and goes with the block entity when it is removed, replaced or
//! unloaded. The Part object is shared behaviour and owns no instance's value.

use crate::*;
use std::{cell::RefCell, fmt, sync::Arc};

/// A block entity Part's typed state: one value per block entity of the Part's type,
/// owned by the game, not by the Part or a mod's statics.
///
/// Implement it beside [`BlockEntityPart`] and attach the Part with
/// [`Registrar::block_entity_part_with_state`](crate::Registrar). `BlockEntityPart`
/// itself stays object-safe: this trait is only for the typed calls below.
///
/// ```rust,ignore
/// struct Press {
///     progress: AttachmentKey<u32>,
/// }
/// impl BlockEntityPartState for Press {
///     type Data = u32;
///     fn state_key(&self) -> &AttachmentKey<u32> {
///         &self.progress
///     }
/// }
/// // In a hook with `host: &mut dyn ServerHost` and the block entity's `entity`:
/// let done = self.edit_state(host, entity, |_, progress| {
///     *progress += 1;
///     *progress >= 200
/// })?;
/// ```
///
/// Reads take `&dyn Host` and return a snapshot. Edits take `&mut dyn ServerHost`, run in
/// their own nested transaction and store the new value only when the closure returns.
/// Every call first checks that `self.state_key()` belongs to the active installation and
/// that `entity` is still the block entity it named, in the level this host is in: a
/// replaced, removed or unloaded one refuses with [`PartStateError::Stale`]. A block entity
/// out of the world for its own tick still answers, through
/// [`BlockEntitiesHost::block_entity_identity`].
///
/// Server-side only for now: the state key must not sync, and a client's copy holds nothing
/// of it, so a client reads the default.
pub trait BlockEntityPartState: BlockEntityPart + Sized {
    /// The value each block entity of the Part's type keeps.
    type Data: Clone + Send + Sync + 'static;
    /// The issued attachment key this Part keeps its per-block-entity state under.
    /// Register its [`AttachmentDescriptor`](crate::AttachmentDescriptor) during
    /// [`Hooks::register`](crate::Hooks::register) (or look up another package's key with
    /// [`Registrar::attachment_key`](crate::Registrar)) and store it on the Part; without a
    /// persistence codec the value is kept for the session only.
    fn state_key(&self) -> &AttachmentKey<Self::Data>;

    /// The block entity's value, or the default when it has none. Inserts nothing.
    fn state(&self, host: &dyn Host, entity: BlockEntityRef) -> Result<Self::Data, PartStateError> {
        let key = self.state_key();
        Ok(self
            .state_existing(host, entity)?
            .unwrap_or_else(|| key.descriptor().default_value()))
    }

    /// The block entity's value, or `None` when it has none. Inserts nothing.
    fn state_existing(
        &self,
        host: &dyn Host,
        entity: BlockEntityRef,
    ) -> Result<Option<Self::Data>, PartStateError> {
        read_state(host, entity, self.state_key())
    }

    /// The value through a read-only level, or its default. Inserts nothing.
    /// No chunk loading or loot generation; unloaded/stale holders fail, clients read the
    /// default. Owning game thread only, never reachable from worker-thread hook bodies.
    fn state_in(
        &self,
        level: &dyn crate::LevelHost,
        entity: BlockEntityRef,
    ) -> Result<Self::Data, PartStateError> {
        let key = self.state_key();
        Ok(self
            .state_existing_in(level, entity)?
            .unwrap_or_else(|| key.descriptor().default_value()))
    }
    /// The existing value through a read-only level. No chunk loading or loot generation;
    /// unloaded/stale holders fail and clients return None. Owning game thread only, never
    /// reachable from worker-thread hook bodies.
    fn state_existing_in(
        &self,
        level: &dyn crate::LevelHost,
        entity: BlockEntityRef,
    ) -> Result<Option<Self::Data>, PartStateError> {
        let key = self.state_key();
        let checked = key.live().map_err(|error| attachment(key, error))?;
        level
            .block_entity_part_snapshot(entity, checked.namespace(), checked.name())?
            .query_checked(checked)
            .get(AttachmentHolder::BlockEntity(entity))
            .map_err(|error| match error {
                AttachmentRowError::Value(error) => attachment(key, error),
                _ => PartStateError::Stale,
            })
    }

    /// Changes the block entity's value with `edit`, starting from the default when it has
    /// none, and stores the result: see [`edit_part_state`].
    fn edit_state<R>(
        &self,
        host: &mut dyn ServerHost,
        entity: BlockEntityRef,
        edit: impl FnOnce(&mut dyn ServerHost, &mut Self::Data) -> R,
    ) -> Result<R, PartStateError> {
        edit_part_state(host, entity, self.state_key(), edit)
    }
}

/// Why a block entity's or an entity's Part state could not be read or changed. Nothing was
/// stored.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[derive(sunrise_mod_api_macros::Wire)]
pub enum PartStateError {
    /// The handle names a block entity or an entity of another level than the one this host
    /// is in.
    OtherLevel,
    /// The holder was removed, replaced or unloaded, or its world closed: get a fresh
    /// handle. A reloaded holder keeps its saved value under a new handle. An entity that
    /// changed level without a player riding it is rebuilt under a new id, and its state goes
    /// with it: find it again by its UUID. Inside an action that is rolled back after the
    /// entity changed level, the state stays with the entity under its new id, and the old
    /// handle stays `Stale`.
    Stale,
    /// The entity is out of its level's lists for its own tick, so its state can't be reached
    /// until that tick ends.
    HolderBusy,
    /// An edit of this holder's same state is already running further up the call:
    /// the inner one is refused rather than overwritten by the outer one.
    Reentrant,
    /// Something else changed the value while the edit's closure ran; the edit stored
    /// nothing and its transaction was aborted.
    Changed,
    /// The attachment store refused the value: it does not decode (its saved bytes are
    /// kept), does not encode, or the key holds another Rust type.
    Attachment {
        /// The state's namespace.
        namespace: &'static str,
        /// The state's name.
        name: &'static str,
        /// What the store said.
        error: AttachmentError,
    },
}

impl fmt::Display for PartStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OtherLevel => f.write_str("the holder is in another level than this host"),
            Self::Stale => f.write_str("the holder was removed, replaced, unloaded or moved"),
            Self::HolderBusy => f.write_str("the entity is out of its level for its own tick"),
            Self::Reentrant => f.write_str("this holder's state is already being edited"),
            Self::Changed => f.write_str("the state changed while it was being edited"),
            Self::Attachment {
                namespace,
                name,
                error,
            } => write!(f, "part state {namespace}:{name}: {error:?}"),
        }
    }
}

impl std::error::Error for PartStateError {}

/// Why a Part's state declaration was refused at registration.
#[derive(sunrise_mod_api_macros::Wire, Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PartStateRefusal {
    /// The state key's sync codec is set: Part state is not sent to clients yet.
    Sync,
    /// The state key's `copy_on_death` policy is set: a block entity has no death to copy across.
    CopyOnDeath,
    /// The state key was not issued by the active registration pass (for example, it was kept
    /// from a previous installation).
    StaleKey,
    /// The entity type keeps no entity Part state: a player, a vanilla family no entity part
    /// runs on, or no type at all. Decided once every type is known, when the game loads.
    EntityKind {
        /// The entity type id the Part was attached to.
        kind: &'static str,
        /// Why that type is refused.
        reason: &'static str,
    },
}

impl PartStateRefusal {
    /// What the refusal says, for a holder named `holder` (`block entities`, `entities`).
    pub(crate) fn describe(&self, f: &mut fmt::Formatter<'_>, holder: &str) -> fmt::Result {
        match self {
            Self::Sync => f.write_str("sync is not supported for part state yet"),
            Self::CopyOnDeath => write!(f, "copy_on_death does not apply to {holder}"),
            Self::StaleKey => {
                f.write_str("the attachment key was not issued in this registration pass")
            }
            Self::EntityKind { kind, reason } => write!(f, "entity type {kind}: {reason}"),
        }
    }
}

/// A Part with state could not be attached.
#[derive(sunrise_mod_api_macros::Wire, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlockEntityStateError {
    /// The Part itself was refused, as by `block_entity_part`.
    Part(BlockEntityPartError),
    /// Its state declaration was refused.
    State {
        /// The Part type.
        part: &'static str,
        /// The state's namespace.
        namespace: &'static str,
        /// The state's name.
        name: &'static str,
        /// The state's Rust type.
        data: &'static str,
        /// Why.
        reason: PartStateRefusal,
    },
}

impl fmt::Display for BlockEntityStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self::State {
            part,
            namespace,
            name,
            data,
            reason,
        } = self
        else {
            let Self::Part(error) = self else {
                unreachable!()
            };
            return error.fmt(f);
        };
        write!(
            f,
            "part {part}: state {namespace}:{name} of type {data} refused: "
        )?;
        reason.describe(f, "block entities")
    }
}

impl std::error::Error for BlockEntityStateError {}

impl From<BlockEntityPartError> for BlockEntityStateError {
    fn from(error: BlockEntityPartError) -> Self {
        Self::Part(error)
    }
}

/// One Part state declaration, as the registrar checks it.
// no wire: host-local registration record carrying an issued attachment key's identity; never crosses the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PartStateDeclaration {
    /// The state's namespace.
    pub namespace: &'static str,
    /// The state's name.
    pub name: &'static str,
    /// The state type's name, which a refusal names.
    pub data_name: &'static str,
    sync: bool,
    copy_on_death: bool,
    identity: crate::parts::AttachmentIdentity,
}

impl PartStateDeclaration {
    /// The declaration of `key`: what the issued key is erased to, for registration.
    pub fn of<T: Send + Sync + 'static>(key: &AttachmentKey<T>) -> Self {
        Self {
            namespace: key.namespace(),
            name: key.name(),
            data_name: std::any::type_name::<T>(),
            sync: key.is_synced(),
            copy_on_death: key.policy().copy_on_death,
            identity: key.identity(),
        }
    }

    /// Validate that the state is local to its owning holder.
    pub fn check(&self) -> Result<(), PartStateRefusal> {
        if self.sync {
            return Err(PartStateRefusal::Sync);
        }
        if self.copy_on_death {
            return Err(PartStateRefusal::CopyOnDeath);
        }
        Ok(())
    }

    /// The error refusing it for `reason`, for entity Part `part` attached to `kind`.
    pub fn refused_on_entity(
        &self,
        part: &'static str,
        kind: &'static str,
        reason: PartStateRefusal,
    ) -> crate::EntityPartStateError {
        crate::EntityPartStateError::State {
            part,
            kind,
            namespace: self.namespace,
            name: self.name,
            data: self.data_name,
            reason,
        }
    }

    /// The error refusing it for `reason`, for Part `part`.
    pub fn refused(&self, part: &'static str, reason: PartStateRefusal) -> BlockEntityStateError {
        BlockEntityStateError::State {
            part,
            namespace: self.namespace,
            name: self.name,
            data: self.data_name,
            reason,
        }
    }
}

/// Whether `record` is the exact registration that issued `declaration`'s attachment key.
// hidden: registrar admission check between sunrise-mod-vocab and sunrise-mod-api.
#[doc(hidden)]
pub fn part_state_matches_registration(
    declaration: &PartStateDeclaration,
    record: &RegisteredAttachment,
) -> bool {
    record.matches_identity(&declaration.identity)
}

/// A holder Part state is kept on: a scoped handle that a call checks against the host before
/// it reaches the holder's attachment map. Sealed: the game decides which holders keep it.
pub trait PartStateHolder: Copy + PartialEq + private::Sealed + 'static {
    /// Checks the handle against this host's level and the holder it names now, and gives the
    /// level-relative holder its value is stored under.
    // hidden: the Part state core's own step; mods call `state` and `edit_state`
    #[doc(hidden)]
    fn part_state_holder(self, host: &dyn Host) -> Result<Holder, PartStateError>;
    /// The handle as the attachment index names it.
    // hidden: the Part state core's own step; mods call `state` and `edit_state`
    #[doc(hidden)]
    fn attachment_holder(self) -> AttachmentHolder;
}

mod private {
    pub trait Sealed {}
    impl Sealed for crate::BlockEntityRef {}
    impl Sealed for crate::EntityRef {}
}

impl PartStateHolder for BlockEntityRef {
    fn part_state_holder(self, host: &dyn Host) -> Result<Holder, PartStateError> {
        if host.level(host.dimension()) != Some(self.level) {
            return Err(PartStateError::OtherLevel);
        }
        if host.block_entity_identity(self.level, self.position) != Some(self) {
            return Err(PartStateError::Stale);
        }
        Ok(Holder::BlockEntity(self.position))
    }

    fn attachment_holder(self) -> AttachmentHolder {
        AttachmentHolder::BlockEntity(self)
    }
}

fn attachment<T: 'static>(key: &AttachmentKey<T>, error: AttachmentError) -> PartStateError {
    match error {
        AttachmentError::MissingHolder => PartStateError::Stale,
        error => PartStateError::Attachment {
            namespace: key.namespace(),
            name: key.name(),
            error,
        },
    }
}

/// [`BlockEntityPartState::state_existing`] for any issued key.
pub fn read_state<H: PartStateHolder, T: Clone + Send + Sync + 'static>(
    host: &dyn Host,
    entity: H,
    key: &AttachmentKey<T>,
) -> Result<Option<T>, PartStateError> {
    let checked = key.live().map_err(|error| attachment(key, error))?;
    entity.part_state_holder(host)?;
    host.attachment_query(checked.namespace(), checked.name())
        .map_err(|error| attachment(key, error))?
        .query_checked(checked)
        .get(entity.attachment_holder())
        .map_err(|error| match error {
            AttachmentRowError::Value(error) => attachment(key, error),
            _ => PartStateError::Stale,
        })
}

thread_local! {
    /// The edits running on this thread, innermost last.
    static EDITING: RefCell<Vec<(AttachmentHolder, &'static str, &'static str)>> =
        const { RefCell::new(Vec::new()) };
}

/// One running edit, taken off [`EDITING`] however the edit ends.
struct Editing;

impl Editing {
    fn enter<T: 'static>(
        entity: AttachmentHolder,
        key: &AttachmentKey<T>,
    ) -> Result<Self, PartStateError> {
        EDITING.with_borrow_mut(|editing| {
            let (namespace, name) = key.parts();
            let this = (entity, namespace, name);
            if editing.contains(&this) {
                return Err(PartStateError::Reentrant);
            }
            editing.push(this);
            Ok(Self)
        })
    }
}

impl Drop for Editing {
    fn drop(&mut self) {
        EDITING.with_borrow_mut(|editing| editing.pop());
    }
}

/// [`BlockEntityPartState::edit_state`] or [`crate::EntityPartState::edit_state`] for any issued
/// key.
///
/// Runs in a transaction of its own, nested in the open one if any: the value is read
/// (decoded once from its saved form, or the default when absent), lent to `edit` as a
/// copy while no store borrow is held, and stored back after `edit` returns, if the holder
/// is still the one `entity` names and nothing else replaced the value meanwhile.
/// Otherwise, or when the value does not encode, the transaction aborts with everything
/// `edit` wrote and the old value stays, with its saved bytes. A panic in `edit` aborts it
/// too. An undecodable saved value refuses before `edit` runs and is kept as it was.
/// An edit of the same holder and key from inside `edit` is refused.
pub fn edit_part_state<H: PartStateHolder, T: Clone + Send + Sync + 'static, R>(
    host: &mut dyn ServerHost,
    entity: H,
    key: &AttachmentKey<T>,
    edit: impl FnOnce(&mut dyn ServerHost, &mut T) -> R,
) -> Result<R, PartStateError> {
    let checked = key.live().map_err(|error| attachment(key, error))?;
    let _editing = Editing::enter(entity.attachment_holder(), key)?;
    host.transaction(|host, _| {
        let at = entity.part_state_holder(host)?;
        let store = host.attachments();
        let current = checked
            .get_existing(store, at)
            .map_err(|error| attachment(key, error))?;
        // The value's own allocation: each store makes a new one and a transaction puts
        // the same one back, so it changed exactly when this differs. Holding it keeps
        // its address from being reused.
        let revision = store
            .decoded(at, checked.namespace(), checked.name())
            .map_err(|error| attachment(key, error))?;
        let mut value = current.unwrap_or_else(|| checked.default_value());
        let answer = edit(host, &mut value);
        entity.part_state_holder(host)?;
        let store = host.attachments();
        let now = store
            .decoded(at, checked.namespace(), checked.name())
            .map_err(|error| attachment(key, error))?;
        if !same(revision.as_ref(), now.as_ref()) {
            return Err(PartStateError::Changed);
        }
        checked
            .set(store, at, value)
            .map_err(|error| attachment(key, error))?;
        Ok(answer)
    })
}

fn same(a: Option<&AttachmentValue>, b: Option<&AttachmentValue>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(sync: bool, copy_on_death: bool, codec: bool) -> AttachmentKey<i32> {
        const CODEC: AttachmentCodec<i32> = AttachmentCodec {
            encode: |_| Ok(Compound::new()),
            decode: |_| Ok(0),
        };
        let mut descriptor = AttachmentDescriptor::new("test:state", || 0);
        if codec {
            descriptor = descriptor.serialize(CODEC);
        }
        if sync {
            descriptor = descriptor.sync(CODEC);
        }
        if copy_on_death {
            descriptor = descriptor.copy_on_death();
        }
        crate::parts::registered_attachment_fixture("test", descriptor).1
    }

    #[test]
    fn declarations_refuse_sync_and_copy_on_death() {
        let check = |key: &AttachmentKey<i32>| PartStateDeclaration::of(key).check();
        assert_eq!(check(&key(false, false, true)), Ok(()));
        assert_eq!(check(&key(false, false, false)), Ok(()));
        assert_eq!(check(&key(true, false, true)), Err(PartStateRefusal::Sync));
        assert_eq!(check(&key(true, false, false)), Err(PartStateRefusal::Sync));
        assert_eq!(
            check(&key(false, true, true)),
            Err(PartStateRefusal::CopyOnDeath)
        );
    }

    #[test]
    fn read_and_edit_part_state_refuse_stale_keys() {
        let first = crate::parts::install_attachment_fixture(
            "test",
            AttachmentDescriptor::new("test:state", || 0_i32),
        );
        let _second = crate::parts::install_attachment_fixture(
            "test",
            AttachmentDescriptor::new("test:state", || 0_i32),
        );
        let mut host = crate::testing::FakeHost::default();
        let entity = BlockEntityRef {
            level: host.level.level(),
            position: [0, 64, 0],
            generation: 1,
        };
        let expected = Err(PartStateError::Attachment {
            namespace: "test",
            name: "state",
            error: AttachmentError::StaleKey,
        });
        assert_eq!(read_state(&host, entity, &first), expected);
        assert_eq!(
            edit_part_state(&mut host, entity, &first, |_, _| panic!("ran")),
            expected
        );
    }

    #[test]
    fn revisions_compare_allocations_not_values() {
        let a: AttachmentValue = Arc::new(7_i32);
        let b: AttachmentValue = Arc::new(7_i32);
        assert!(same(Some(&a), Some(&a.clone())));
        assert!(!same(Some(&a), Some(&b)));
        assert!(same(None, None));
        assert!(!same(Some(&a), None));
    }
}
