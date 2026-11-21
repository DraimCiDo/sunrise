//! Typed native-mod data attached to server-owned holders.
#[doc(hidden)]
pub mod index;
#[doc(hidden)]
mod index_key;
mod lifetime;
#[doc(hidden)]
pub mod observer;
mod query;
mod resident;
#[doc(hidden)]
pub mod snapshot;

pub use query::{
    AttachmentBatch, AttachmentIter, AttachmentQuery, AttachmentRowError, AttachmentRowRef,
};

#[doc(hidden)]
pub use lifetime::{AttachmentLifetime, HasAttachmentLifetime};
#[doc(hidden)]
pub use resident::ResidentAttachments;

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    fmt,
    hash::Hash,
    sync::{Arc, LazyLock, Mutex, OnceLock, Weak},
};

use sunrise_core::nbt::{Compound, JavaString, Tag};

// A borrowed comparison view preserves Java's UTF-16 ordering without building keys.
trait AttachmentKeyView {
    fn namespace(&self) -> Utf16Units<'_>;
    fn name(&self) -> Utf16Units<'_>;
}
enum Utf16Units<'a> {
    Owned(std::iter::Copied<std::slice::Iter<'a, u16>>),
    Borrowed(std::str::EncodeUtf16<'a>),
}
impl Utf16Units<'_> {
    fn cmp(self, other: Self) -> std::cmp::Ordering {
        // Select the concrete iterators once, not for every UTF-16 unit.
        match (self, other) {
            (Self::Owned(left), Self::Owned(right)) => left.cmp(right),
            (Self::Owned(left), Self::Borrowed(right)) => left.cmp(right),
            (Self::Borrowed(left), Self::Owned(right)) => left.cmp(right),
            (Self::Borrowed(left), Self::Borrowed(right)) => left.cmp(right),
        }
    }
}
impl AttachmentKeyView for (JavaString, JavaString) {
    fn namespace(&self) -> Utf16Units<'_> {
        Utf16Units::Owned(self.0.0.iter().copied())
    }
    fn name(&self) -> Utf16Units<'_> {
        Utf16Units::Owned(self.1.0.iter().copied())
    }
}
impl AttachmentKeyView for (&str, &str) {
    fn namespace(&self) -> Utf16Units<'_> {
        Utf16Units::Borrowed(self.0.encode_utf16())
    }
    fn name(&self) -> Utf16Units<'_> {
        Utf16Units::Borrowed(self.1.encode_utf16())
    }
}
impl PartialEq for dyn AttachmentKeyView + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for dyn AttachmentKeyView + '_ {}
impl PartialOrd for dyn AttachmentKeyView + '_ {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for dyn AttachmentKeyView + '_ {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.namespace()
            .cmp(other.namespace())
            .then_with(|| self.name().cmp(other.name()))
    }
}
impl<'a> std::borrow::Borrow<dyn AttachmentKeyView + 'a> for (JavaString, JavaString) {
    fn borrow(&self) -> &(dyn AttachmentKeyView + 'a) {
        self
    }
}

// no wire: owned by the lower runtime crate, which cannot depend on mod_api's derive;
// mod_api supplies the explicit tagged Wire implementation, including bounded static text.
/// An attachment operation could not be completed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttachmentError {
    /// The holder does not exist or is unavailable on this side.
    MissingHolder,
    /// The key was used with a different Rust type.
    TypeMismatch,
    /// The saved value could not be decoded.
    Decode,
    /// The value could not be encoded.
    Encode,
    /// A namespace in the saved compound is not a compound.
    InvalidNamespace,
    /// Copy-on-death requires a serializer.
    InvalidCopyPolicy,
    /// Attachment sync policy does not match its load-time declaration.
    InvalidSyncPolicy,
    /// The holder's physical storage is temporarily extracted or lifted.
    /// Retry after the current pass; the refused store writes no value.
    HolderBusy,
    /// This physical holder family has no attachment lifetime storage; retrying cannot help.
    /// Stable lowercase family names: mob, item, orb, projectile, armorstand,
    /// itemframe, painting, boat, minecart, other, fangs, crystal, dragon, tnt,
    /// falling, rocket, trident, spawner, cloud, cushion, knot, player, hook,
    /// dragonpart.
    UnsupportedHolder(&'static str),
    /// No loaded package declared this attachment id.
    Undeclared {
        package_id: String,
        attachment_id: String,
    },
    /// This attachment key belongs to a previous installation and is no longer live.
    StaleKey,
}

/// NBT key reserved for server-owned attachment maps.
pub const ATTACHMENTS_KEY: &str = "sunrise:attachments";

/// Read the reserved compound without replacing an invalid saved value.
pub fn attachment_compound(tag: Option<&Tag>) -> Result<Option<&Compound>, AttachmentError> {
    match tag {
        None => Ok(None),
        Some(Tag::Compound(compound)) => Ok(Some(compound)),
        Some(_) => Err(AttachmentError::InvalidNamespace),
    }
}

/// A server holder in the host's current dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
// no wire: runtime-owned selector; mod_api supplies its explicit tagged Wire codec.
pub enum Holder {
    Entity(u32),
    Player(u128),
    BlockEntity([i32; 3]),
    Chunk([i32; 2]),
    Level,
    /// A detached player's map, available only during its clone hook.
    OriginalPlayer(u64),
}

/// Whether borrowing a map may create publication authority for a new value.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
// no wire: host-local storage access intent, never a cross-target payload.
pub enum AttachmentAccess {
    /// Inspection, removal or sync: never create a registration.
    Existing,
    /// A validated value is about to be stored, including an inserted default.
    Store,
}

/// The game's access to each holder's map: every [`AttachmentStore`] answers through it.
pub trait AttachmentHost {
    /// Validate the load-owned declaration before accessing storage.
    fn validate_attachment(
        &self,
        namespace: &str,
        name: &str,
        sync: bool,
    ) -> Result<(), AttachmentError>;
    fn attachments(
        &mut self,
        holder: Holder,
        access: AttachmentAccess,
    ) -> Result<&mut Attachments, AttachmentError>;
}

/// A value of a mod's own type, as the holder keeps it: shared, so a copy of the holder's map
/// copies no value.
// no wire: host-local erased Arc value; attachment values cross through their registered codec.
pub type AttachmentValue = Arc<dyn Any + Send + Sync>;

/// How a policy-checked type declared its value: what saving, respawning and syncing do with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// no wire: runtime-owned policy; mod_api supplies its explicit field-wise Wire codec.
pub struct AttachmentPolicy {
    pub copy_on_death: bool,
    pub sync: bool,
}

/// What comes with a stored value: its saved form (`None` for a type with no persistence codec),
/// its synced form (`None` for a type with no sync codec), its type's policy, and why it is
/// stored.
#[derive(Clone, Debug, PartialEq)]
// no wire: runtime-owned saved form; mod_api supplies its explicit NBT-aware Wire codec.
pub struct AttachmentForm {
    pub encoded: Option<Compound>,
    pub synced: Option<Compound>,
    pub policy: AttachmentPolicy,
    pub write: AttachmentWrite,
}

/// Why a value is stored, which decides what else changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
// no wire: runtime-owned write intent; mod_api supplies its explicit tagged Wire codec.
pub enum AttachmentWrite {
    /// A saved value just decoded: nothing else changes.
    Decoded,
    /// The type's default, inserted by a read: clients hear of it if the type syncs.
    Default,
    /// A value a mod set: clients hear of it if the type syncs, and the saved form it replaces
    /// is dropped.
    Set,
}

/// The current server's holders' values, erased to what crosses any boundary: typed
/// attachment key methods (`AttachmentKey`) are built on it, for mods.
pub trait AttachmentStore {
    /// Validate an attachment type against the loaded declarations.
    fn validate_attachment(
        &mut self,
        namespace: &str,
        name: &str,
        sync: bool,
    ) -> Result<(), AttachmentError>;
    /// Whether `holder` keeps a value under `namespace:name`, saved or decoded.
    fn holds(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<bool, AttachmentError>;
    /// The value under `namespace:name` if it has been decoded.
    fn decoded(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<Option<AttachmentValue>, AttachmentError>;
    /// The saved form under `namespace:name` if it hasn't been decoded yet.
    fn saved(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<Option<Compound>, AttachmentError>;
    /// The synced form under `namespace:name` if it arrived from the server and hasn't been
    /// decoded yet.
    fn synced_raw(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<Option<Compound>, AttachmentError>;
    /// Keep `value` under `namespace:name`, with what comes with it.
    fn store(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
        value: AttachmentValue,
        form: AttachmentForm,
    ) -> Result<(), AttachmentError>;
    /// Drop the value under `namespace:name`, saved or decoded; clients hear of it if `sync`.
    fn discard(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
        sync: bool,
    ) -> Result<(), AttachmentError>;
    /// Send the value under `namespace:name` to the clients that see the holder again.
    fn queue_sync(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<(), AttachmentError>;
}

impl<H: AttachmentHost + ?Sized> AttachmentStore for H {
    fn validate_attachment(
        &mut self,
        namespace: &str,
        name: &str,
        sync: bool,
    ) -> Result<(), AttachmentError> {
        AttachmentHost::validate_attachment(self, namespace, name, sync)
    }
    fn holds(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<bool, AttachmentError> {
        AttachmentHost::validate_attachment(self, namespace, name, false)?;
        Ok(Attachments::holds(
            self.attachments(holder, AttachmentAccess::Existing)?,
            namespace,
            name,
        ))
    }
    fn decoded(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<Option<AttachmentValue>, AttachmentError> {
        AttachmentHost::validate_attachment(self, namespace, name, false)?;
        Attachments::decoded(
            self.attachments(holder, AttachmentAccess::Existing)?,
            namespace,
            name,
        )
    }
    fn saved(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<Option<Compound>, AttachmentError> {
        AttachmentHost::validate_attachment(self, namespace, name, false)?;
        Attachments::saved(
            self.attachments(holder, AttachmentAccess::Existing)?,
            namespace,
            name,
        )
    }
    fn synced_raw(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<Option<Compound>, AttachmentError> {
        AttachmentHost::validate_attachment(self, namespace, name, false)?;
        Attachments::synced_raw(
            self.attachments(holder, AttachmentAccess::Existing)?,
            namespace,
            name,
        )
    }
    fn store(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
        value: AttachmentValue,
        form: AttachmentForm,
    ) -> Result<(), AttachmentError> {
        AttachmentHost::validate_attachment(self, namespace, name, form.policy.sync)?;
        let AttachmentForm {
            encoded,
            synced,
            policy,
            write,
        } = form;
        if matches!(holder, Holder::OriginalPlayer(_)) {
            return Err(AttachmentError::MissingHolder);
        }
        Attachments::store(
            self.attachments(holder, AttachmentAccess::Store)?,
            namespace,
            name,
            Decoded {
                value,
                encoded,
                synced,
                policy,
            },
            write,
        );
        Ok(())
    }
    fn discard(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
        sync: bool,
    ) -> Result<(), AttachmentError> {
        AttachmentHost::validate_attachment(self, namespace, name, sync)?;
        if matches!(holder, Holder::OriginalPlayer(_)) {
            return Err(AttachmentError::MissingHolder);
        }
        Attachments::discard(
            self.attachments(holder, AttachmentAccess::Existing)?,
            namespace,
            name,
            sync,
        );
        Ok(())
    }
    fn queue_sync(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<(), AttachmentError> {
        AttachmentHost::validate_attachment(self, namespace, name, true)?;
        Attachments::queue_sync(
            self.attachments(holder, AttachmentAccess::Existing)?,
            namespace,
            name,
        );
        Ok(())
    }
}

/// The optional on-disk or network representation of an attachment.
// no wire: host-local encoder/decoder function pointers, not cross-target values.
pub struct AttachmentCodec<T> {
    pub encode: fn(&T) -> Result<Compound, AttachmentError>,
    pub decode: fn(&Compound) -> Result<T, AttachmentError>,
}

impl<T> Clone for AttachmentCodec<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for AttachmentCodec<T> {}

/// A mod-defined attachment key and its value's behavior.
// no wire: host-local key with callback pointers; attachment calls cross by namespace/name and codec.
pub struct AttachmentType<T: 'static> {
    pub namespace: &'static str,
    pub name: &'static str,
    pub default: fn() -> T,
    /// Optional persistence codec for saving and loading on disk.
    pub codec: Option<AttachmentCodec<T>>,
    pub copy_on_death: bool,
    /// Optional sync codec for sending the value to clients that see the holder;
    /// independent of persistence `codec`.
    pub sync_codec: Option<AttachmentCodec<T>>,
}

impl<T> AttachmentType<T> {
    fn policy(&self) -> AttachmentPolicy {
        AttachmentPolicy {
            copy_on_death: self.copy_on_death,
            sync: self.sync_codec.is_some(),
        }
    }
    fn decode_saved(&self, saved: &Compound) -> Result<T, AttachmentError> {
        (self.codec.as_ref().ok_or(AttachmentError::Decode)?.decode)(saved)
    }
    fn decode_synced(&self, synced: &Compound) -> Result<T, AttachmentError> {
        (self
            .sync_codec
            .as_ref()
            .ok_or(AttachmentError::Decode)?
            .decode)(synced)
    }
    fn validate_policy(&self) -> Result<(), AttachmentError> {
        if self.copy_on_death && self.codec.is_none() {
            return Err(AttachmentError::InvalidCopyPolicy);
        }
        Ok(())
    }
}

/// An attachment type whose namespace, name, and copy policy have already been validated.
///
/// Syncing (`sync_codec`) is independent of persistence (`codec`), so a sync-only attachment
/// with no persistence codec is valid.
// no wire: host-local checked descriptor with callback pointers; attachment calls cross by id and codec.
pub struct CheckedAttachmentType<T: 'static> {
    raw: AttachmentType<T>,
}

impl<T: 'static> CheckedAttachmentType<T> {
    /// Validate `raw`'s copy policy once.
    pub fn new(raw: AttachmentType<T>) -> Result<Self, AttachmentError> {
        raw.validate_policy()?;
        Ok(Self { raw })
    }
    /// Borrow the underlying unchecked descriptor.
    pub fn as_type(&self) -> &AttachmentType<T> {
        &self.raw
    }
    pub fn namespace(&self) -> &'static str {
        self.raw.namespace
    }
    pub fn name(&self) -> &'static str {
        self.raw.name
    }
    pub fn default_value(&self) -> T {
        (self.raw.default)()
    }
    pub fn has_codec(&self) -> bool {
        self.raw.codec.is_some()
    }
    pub fn is_persistent(&self) -> bool {
        self.raw.codec.is_some()
    }
    pub fn is_synced(&self) -> bool {
        self.raw.sync_codec.is_some()
    }
    pub fn policy(&self) -> AttachmentPolicy {
        self.raw.policy()
    }
    /// Borrow a value from an immutable attachment entry without re-running policy checks.
    pub fn read<'a>(&self, entry: &'a AttachmentEntry) -> Result<&'a T, AttachmentError> {
        entry.read_validated(&self.raw)
    }
}

impl<T: Clone + Send + Sync + 'static> CheckedAttachmentType<T> {
    /// Read a value, inserting its default if absent except on an original-player snapshot.
    pub fn get<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<T, AttachmentError> {
        AttachmentOps::Checked(self).get(store, holder)
    }
    /// Whether a holder carries a value; never inserts the default.
    pub fn has<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<bool, AttachmentError> {
        AttachmentOps::Checked(self).has(store, holder)
    }
    /// Read a value only if present; a saved or synced value decodes once.
    pub fn get_existing<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<Option<T>, AttachmentError> {
        AttachmentOps::Checked(self).get_existing(store, holder)
    }
    /// Read the decoded value through `read` without cloning it.
    pub fn with_decoded<S: AttachmentStore + ?Sized, R>(
        &self,
        store: &mut S,
        holder: Holder,
        read: impl FnOnce(Option<&T>) -> R,
    ) -> Result<R, AttachmentError> {
        AttachmentOps::Checked(self).with_decoded(store, holder, read)
    }
    /// Replace a value on an existing holder.
    pub fn set<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
        value: T,
    ) -> Result<(), AttachmentError> {
        AttachmentOps::Checked(self).set(store, holder, value)
    }
    /// Remove a value from an existing holder.
    pub fn remove<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<Option<T>, AttachmentError> {
        AttachmentOps::Checked(self).remove(store, holder)
    }
    /// Send the current value to the clients that see the holder.
    pub fn sync_data<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<(), AttachmentError> {
        AttachmentOps::Checked(self).sync_data(store, holder)
    }
}

enum AttachmentOps<'a, T: 'static> {
    Raw(&'a AttachmentType<T>),
    Checked(&'a CheckedAttachmentType<T>),
}

impl<T: 'static> AttachmentOps<'_, T> {
    fn raw(&self) -> &AttachmentType<T> {
        match self {
            Self::Raw(raw) => raw,
            Self::Checked(checked) => &checked.raw,
        }
    }
    fn validate<S: AttachmentStore + ?Sized>(&self, store: &S) -> Result<(), AttachmentError> {
        let raw = self.raw();
        match self {
            Self::Raw(_) => {
                store.validate_attachment(raw.namespace, raw.name, raw.sync_codec.is_some())?;
                raw.validate_policy()
            }
            Self::Checked(_) => Ok(()),
        }
    }
}

impl<T: Clone + Send + Sync + 'static> AttachmentOps<'_, T> {
    fn get<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<T, AttachmentError> {
        if let Some(value) = self.get_existing(store, holder)? {
            return Ok(value);
        }
        let value = (self.raw().default)();
        if !matches!(holder, Holder::OriginalPlayer(_)) {
            self.keep(store, holder, value.clone(), AttachmentWrite::Default)?;
        }
        Ok(value)
    }
    fn has<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<bool, AttachmentError> {
        let raw = self.raw();
        if matches!(self, Self::Raw(_)) {
            store.validate_attachment(raw.namespace, raw.name, raw.sync_codec.is_some())?;
        }
        store.holds(holder, raw.namespace, raw.name)
    }
    fn get_existing<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<Option<T>, AttachmentError> {
        let raw = self.raw();
        self.validate(store)?;
        if let Some(value) = store.decoded(holder, raw.namespace, raw.name)? {
            return value
                .downcast_ref::<T>()
                .cloned()
                .map(Some)
                .ok_or(AttachmentError::TypeMismatch);
        }
        if let Some(saved) = store.saved(holder, raw.namespace, raw.name)? {
            let value = raw.decode_saved(&saved)?;
            if !matches!(holder, Holder::OriginalPlayer(_)) {
                self.keep(store, holder, value.clone(), AttachmentWrite::Decoded)?;
            }
            return Ok(Some(value));
        }
        let Some(synced) = store.synced_raw(holder, raw.namespace, raw.name)? else {
            return Ok(None);
        };
        let value = raw.decode_synced(&synced)?;
        if !matches!(holder, Holder::OriginalPlayer(_)) {
            let form = AttachmentForm {
                encoded: None,
                synced: Some(synced),
                policy: raw.policy(),
                write: AttachmentWrite::Decoded,
            };
            store.store(
                holder,
                raw.namespace,
                raw.name,
                Arc::new(value.clone()),
                form,
            )?;
        }
        Ok(Some(value))
    }
    fn with_decoded<S: AttachmentStore + ?Sized, R>(
        &self,
        store: &mut S,
        holder: Holder,
        read: impl FnOnce(Option<&T>) -> R,
    ) -> Result<R, AttachmentError> {
        let raw = self.raw();
        self.validate(store)?;
        let value = store.decoded(holder, raw.namespace, raw.name)?;
        Ok(read(value.as_ref().and_then(|v| v.downcast_ref::<T>())))
    }
    fn set<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
        value: T,
    ) -> Result<(), AttachmentError> {
        let raw = self.raw();
        self.validate(store)?;
        if let Some(old) = store.decoded(holder, raw.namespace, raw.name)?
            && !old.is::<T>()
        {
            return Err(AttachmentError::TypeMismatch);
        }
        self.keep(store, holder, value, AttachmentWrite::Set)
    }
    fn remove<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<Option<T>, AttachmentError> {
        if !self.has(store, holder)? {
            return Ok(None);
        }
        let raw = self.raw();
        let value = self.get_existing(store, holder);
        store.discard(holder, raw.namespace, raw.name, raw.sync_codec.is_some())?;
        value
    }
    fn sync_data<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<(), AttachmentError> {
        let raw = self.raw();
        self.validate(store)?;
        if raw.sync_codec.is_some() {
            store.queue_sync(holder, raw.namespace, raw.name)?;
        }
        Ok(())
    }
    fn keep<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
        value: T,
        write: AttachmentWrite,
    ) -> Result<(), AttachmentError> {
        let raw = self.raw();
        let encoded = raw
            .codec
            .as_ref()
            .map(|codec| (codec.encode)(&value))
            .transpose()?;
        let synced = raw
            .sync_codec
            .as_ref()
            .map(|codec| (codec.encode)(&value))
            .transpose()?;
        let value: AttachmentValue = Arc::new(value);
        let form = AttachmentForm {
            encoded,
            synced,
            policy: raw.policy(),
            write,
        };
        store.store(holder, raw.namespace, raw.name, value, form)
    }
}

/// One attachment's new state for a client; `None` means removed.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentSync {
    pub namespace: JavaString,
    pub name: JavaString,
    pub value: Option<Compound>,
}

impl<T: Clone + Send + Sync + 'static> AttachmentType<T> {
    /// Read a value, inserting its default if absent except on an original-player snapshot;
    /// see `get_existing`.
    pub fn get<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<T, AttachmentError> {
        AttachmentOps::Raw(self).get(store, holder)
    }
    /// Whether a holder carries a value, like NeoForge's `hasData`; never inserts the default.
    pub fn has<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<bool, AttachmentError> {
        AttachmentOps::Raw(self).has(store, holder)
    }
    /// Read a value only if present, like `getExistingData`; a saved or synced value decodes once.
    pub fn get_existing<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<Option<T>, AttachmentError> {
        AttachmentOps::Raw(self).get_existing(store, holder)
    }
    /// Read the decoded value through `read` without cloning it: what a read of one part of a
    /// large value costs. A saved value that has not decoded yet is not decoded, so `read` sees
    /// none where `get_existing` would decode one.
    pub fn with_decoded<S: AttachmentStore + ?Sized, R>(
        &self,
        store: &mut S,
        holder: Holder,
        read: impl FnOnce(Option<&T>) -> R,
    ) -> Result<R, AttachmentError> {
        AttachmentOps::Raw(self).with_decoded(store, holder, read)
    }
    /// Replace a value on an existing holder. A key already decoded as a different type is
    /// rejected.
    pub fn set<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
        value: T,
    ) -> Result<(), AttachmentError> {
        AttachmentOps::Raw(self).set(store, holder, value)
    }
    /// Remove a value from an existing holder. A saved value that does not decode is removed
    /// too, and its error returned.
    pub fn remove<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<Option<T>, AttachmentError> {
        AttachmentOps::Raw(self).remove(store, holder)
    }
    /// Send the current value to the clients that see the holder.
    pub fn sync_data<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<(), AttachmentError> {
        AttachmentOps::Raw(self).sync_data(store, holder)
    }
}

/// A decoded value with its saved and synced forms and its type's policy.
struct Decoded {
    value: AttachmentValue,
    encoded: Option<Compound>,
    synced: Option<Compound>,
    policy: AttachmentPolicy,
}

/// The saved form of a decoded value: its persistence encoding and the policies it was
/// declared with. `None` when the type has no persistence codec.
fn envelope(value: &Decoded) -> Option<Compound> {
    value.encoded.as_ref().map(|encoded| {
        let mut envelope = Compound::new();
        envelope.insert("value".into(), Tag::Compound(encoded.clone()));
        if value.policy.copy_on_death {
            envelope.insert("copy_on_death".into(), Tag::Byte(1));
        }
        if value.policy.sync {
            envelope.insert("sync".into(), Tag::Byte(1));
            if let Some(synced) = &value.synced
                && synced != encoded
            {
                envelope.insert("sync_value".into(), Tag::Compound(synced.clone()));
            }
        }
        envelope
    })
}
fn raw_value(raw: &Compound) -> Option<&Compound> {
    static VALUE: LazyLock<JavaString> = LazyLock::new(|| "value".into());
    match raw.get(&VALUE) {
        Some(Tag::Compound(value)) => Some(value),
        _ => None,
    }
}
fn raw_sync_value(raw: &Compound) -> Option<&Compound> {
    static SYNC_VALUE: LazyLock<JavaString> = LazyLock::new(|| "sync_value".into());
    match raw.get(&SYNC_VALUE) {
        Some(Tag::Compound(value)) => Some(value),
        _ => raw_value(raw),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RawKind {
    Saved,
    Synced,
}

enum Entry {
    Raw {
        saved: Compound,
        kind: RawKind,
        reads: ReadCache,
    },
    Decoded(Decoded),
}

/// An immutable attachment binding shared by a holder and its snapshots.
/// Query reads may cache decoded values, but never change the canonical raw/decoded state.
#[doc(hidden)]
#[non_exhaustive]
// no wire: host-local retained binding with erased decoded state, not a wire payload.
pub struct AttachmentEntry {
    state: Entry,
}

// Separate Rust types can read the same raw binding without claiming its canonical type.
// Each successful type gets a stable borrowed value; failed decodes are not cached.
#[derive(Default)]
struct ReadCache {
    value: OnceLock<AttachmentValue>,
    next: OnceLock<Box<ReadCache>>,
}

impl AttachmentEntry {
    /// Rebuild a transferred saved binding for lazy typed reads.
    #[doc(hidden)]
    pub fn from_saved(saved: Compound) -> Self {
        Self::raw(saved)
    }

    /// Capture the saved envelope, if this binding has a codec.
    #[doc(hidden)]
    pub fn saved(&self) -> Option<Compound> {
        match &self.state {
            Entry::Raw { saved, .. } => Some(saved.clone()),
            Entry::Decoded(value) => envelope(value),
        }
    }

    fn raw(saved: Compound) -> Self {
        Self {
            state: Entry::Raw {
                saved,
                kind: RawKind::Saved,
                reads: ReadCache::default(),
            },
        }
    }

    fn raw_synced(value: Compound) -> Self {
        let saved = [
            ("value".into(), Tag::Compound(value)),
            ("sync".into(), Tag::Byte(1)),
        ]
        .into();
        Self::raw_synced_envelope(saved)
    }

    fn raw_synced_envelope(saved: Compound) -> Self {
        Self {
            state: Entry::Raw {
                saved,
                kind: RawKind::Synced,
                reads: ReadCache::default(),
            },
        }
    }
    /// Borrow a value for a query without inserting a default, storing a decoded value,
    /// or changing saved policies or sync state. Decoded bindings never fall back to NBT.
    pub fn read<T: Send + Sync + 'static>(
        &self,
        key: &AttachmentType<T>,
    ) -> Result<&T, AttachmentError> {
        key.validate_policy()?;
        self.read_validated(key)
    }

    /// Borrow a value for a query using a pre-validated attachment type.
    pub fn read_checked<T: Send + Sync + 'static>(
        &self,
        key: &CheckedAttachmentType<T>,
    ) -> Result<&T, AttachmentError> {
        self.read_validated(key.as_type())
    }

    fn read_validated<T: Send + Sync + 'static>(
        &self,
        key: &AttachmentType<T>,
    ) -> Result<&T, AttachmentError> {
        match &self.state {
            Entry::Decoded(value) => value
                .value
                .downcast_ref()
                .ok_or(AttachmentError::TypeMismatch),
            Entry::Raw { saved, kind, reads } => {
                let mut cache = reads;
                loop {
                    let value = match cache.value.get() {
                        Some(value) => value,
                        None => {
                            let payload = raw_value(saved).ok_or(AttachmentError::Decode)?;
                            let decoded = match kind {
                                RawKind::Saved => key.decode_saved(payload)?,
                                RawKind::Synced => key.decode_synced(payload)?,
                            };
                            let value = Arc::new(decoded);
                            // Another reader may have initialized this slot with its type.
                            let _ = cache.value.set(value);
                            cache.value.get().expect("cache initialized")
                        }
                    };
                    if let Some(value) = value.downcast_ref() {
                        return Ok(value);
                    }
                    cache = cache.next.get_or_init(Default::default);
                }
            }
        }
    }
    /// The encoded value, as clients receive it.
    fn synced_value(&self) -> Option<&Compound> {
        match &self.state {
            Entry::Raw {
                saved,
                kind: RawKind::Synced,
                ..
            } => raw_value(saved),
            Entry::Raw {
                saved,
                kind: RawKind::Saved,
                ..
            } => raw_sync_value(saved),
            Entry::Decoded(value) => value.synced.as_ref(),
        }
    }
    fn is_synced(&self) -> bool {
        match &self.state {
            Entry::Raw {
                kind: RawKind::Synced,
                ..
            } => true,
            Entry::Raw {
                saved,
                kind: RawKind::Saved,
                ..
            } => {
                static SYNC: LazyLock<JavaString> = LazyLock::new(|| "sync".into());
                matches!(saved.get(&SYNC), Some(Tag::Byte(1)))
            }
            Entry::Decoded(value) => value.policy.sync,
        }
    }
}

struct AttachmentChange<'a> {
    key: &'a index::AttachmentKey,
    entry: Option<&'a Arc<AttachmentEntry>>,
}

trait AttachmentPublisher: Send + Sync {
    fn publish(&self, changes: &[AttachmentChange<'_>]);
    fn unregister(&self) -> bool;
}

struct IndexedAttachments<H> {
    index: Weak<Mutex<index::AttachmentIndex<H>>>,
    owner: index::AttachmentOwner<H>,
}

impl<H: Copy + Eq + Hash + Send + Sync> AttachmentPublisher for IndexedAttachments<H> {
    fn publish(&self, changes: &[AttachmentChange<'_>]) {
        if let Some(index) = self.index.upgrade() {
            let retired = {
                let mut index = index.lock().unwrap();
                let mut retired = index::AttachmentChange::new(());
                for change in changes {
                    retired.merge(match change.entry {
                        Some(entry) => index.set(&self.owner, change.key, entry.clone()),
                        None => index.remove(&self.owner, change.key),
                    });
                }
                retired
            };
            retired.into_result();
        }
    }

    fn unregister(&self) -> bool {
        self.index.upgrade().is_some_and(|index| {
            let change = {
                let mut index = index.lock().unwrap();
                index.unregister(&self.owner)
            };
            change.into_result()
        })
    }
}

/// Shared authority for one indexed map registration. Cloning or dropping the handle
/// has no effect; authoritative lifetime owners must explicitly unregister it.
#[doc(hidden)]
#[derive(Clone)]
#[non_exhaustive]
pub struct AttachmentRegistration {
    publisher: Arc<dyn AttachmentPublisher>,
}

impl AttachmentRegistration {
    /// Retire only this registration. A stale handle cannot unload its replacement,
    /// and subsequent writes through the retired map cannot restore membership.
    pub fn unregister(&self) -> bool {
        self.publisher.unregister()
    }

    /// Whether two handles refer to the same registration.
    pub fn same_registration(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.publisher, &other.publisher)
    }
}

/// The data held by one entity, block entity, chunk or level.
#[derive(Default)]
// no wire: host-local canonical map and publication authority; only saved values cross the boundary.
pub struct Attachments {
    entries: BTreeMap<index::AttachmentKey, Arc<AttachmentEntry>>,
    // Keep malformed or otherwise unrecognized NBT exactly as it was read.
    // Empty holders need no shared allocation.
    raw: Option<Arc<Compound>>,
    // Keys to send to clients at the next flush; never saved.
    pending: BTreeSet<(JavaString, JavaString)>,
    registration: Option<AttachmentRegistration>,
}

// Save/journal/COW copies are detached. Moves alone preserve publication authority.
impl Clone for Attachments {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            raw: self.raw.clone(),
            pending: self.pending.clone(),
            registration: None,
        }
    }
}

impl fmt::Debug for Attachments {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attachments")
            .field("keys", &self.entries.keys())
            .finish()
    }
}

/// A lone map answers for any holder.
impl AttachmentHost for Attachments {
    fn validate_attachment(
        &self,
        namespace: &str,
        name: &str,
        sync: bool,
    ) -> Result<(), AttachmentError> {
        let _ = (namespace, name, sync);
        Ok(())
    }
    fn attachments(
        &mut self,
        _: Holder,
        _: AttachmentAccess,
    ) -> Result<&mut Attachments, AttachmentError> {
        Ok(self)
    }
}

impl Attachments {
    /// Whether stored attachment memberships exist, excluding preserved unknown NBT.
    #[doc(hidden)]
    pub fn has_index_entries(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Whether this map retains an index publisher. Detached clones do not.
    /// This is not a residency check: storage must rebind on installation or scope
    /// changes, and retire membership explicitly on unload.
    #[doc(hidden)]
    pub fn has_index_binding(&self) -> bool {
        self.registration.is_some()
    }

    /// Detach and retire a publisher when its owning boundary rolls back a binding.
    #[doc(hidden)]
    pub fn unbind_index(&mut self) {
        if let Some(registration) = self.registration.take() {
            registration.unregister();
        }
    }

    /// Detach the publisher without retiring it, for a caller that retires it later: the
    /// index keeps the map's rows as they are now, and later writes no longer reach them.
    #[doc(hidden)]
    pub fn take_index_binding(&mut self) -> Option<AttachmentRegistration> {
        self.registration.take()
    }

    /// Install this authoritative map in key order, invalidating any prior binding
    /// for the holder. Rebind a detached COW map before its first attachment write.
    /// The weak publisher does not keep the dimension alive. Map Drop never unloads;
    /// storage must retire the returned handle or use trusted index `unload`.
    #[doc(hidden)]
    pub fn bind_index<H: Copy + Eq + Hash + Send + Sync + 'static>(
        &mut self,
        index: &Arc<Mutex<index::AttachmentIndex<H>>>,
        holder: H,
    ) -> AttachmentRegistration {
        let change = {
            let mut index = index.lock().unwrap();
            index.register(
                holder,
                self.entries
                    .iter()
                    .map(|(key, entry)| (key.clone(), entry.clone())),
            )
        };
        let owner = change.into_result();
        let registration = AttachmentRegistration {
            publisher: Arc::new(IndexedAttachments {
                index: Arc::downgrade(index),
                owner,
            }),
        };
        self.registration = Some(registration.clone());
        registration
    }

    /// Restore content (including saved data and queued sync) without detaching the
    /// current live registration. Any binding on `replacement` is discarded, not
    /// retired. Unchanged entries keep their index slots and shared row backing.
    #[doc(hidden)]
    pub fn replace_contents(&mut self, mut replacement: Self) {
        replacement.registration = self.registration.take();
        let previous = std::mem::replace(self, replacement);
        if let Some(registration) = &self.registration {
            let changes: Vec<_> = previous
                .entries
                .keys()
                .filter(|key| !self.entries.contains_key(*key))
                .map(|key| AttachmentChange { key, entry: None })
                .chain(self.entries.iter().map(|(key, entry)| AttachmentChange {
                    key,
                    entry: Some(entry),
                }))
                .collect();
            registration.publisher.publish(&changes);
        }
    }

    // All canonical entry mutations publish the same Arc the map keeps. Retain the
    // replaced entry until after publication releases the index lock.
    fn write_entry(&mut self, key: &index::AttachmentKey, entry: Option<Arc<AttachmentEntry>>) {
        let previous = match entry {
            Some(entry) => self.entries.insert(key.clone(), entry),
            None => self.entries.remove(key),
        };
        if let Some(registration) = &self.registration {
            registration.publisher.publish(&[AttachmentChange {
                key,
                entry: self.entries.get(key),
            }]);
        }
        drop(previous);
    }

    /// Read the namespaced attachment compound, leaving unknown data intact.
    pub fn from_nbt(raw: Compound) -> Self {
        let mut result = Self::default();
        for (namespace, tag) in &raw {
            if let Tag::Compound(names) = tag {
                for (name, value) in names {
                    if let Tag::Compound(value) = value {
                        result.entries.insert(
                            (namespace.clone(), name.clone()),
                            Arc::new(AttachmentEntry::raw(value.clone())),
                        );
                    }
                }
            }
        }
        result.raw = (!raw.is_empty()).then(|| Arc::new(raw));
        result
    }
    /// `Entity.load` on an entity whose constructing listeners already set values: each saved
    /// attachment replaces the value there and the others stay, so saved data wins. Unknown
    /// saved data is kept as `from_nbt` keeps it.
    #[doc(hidden)]
    pub fn read_saved_over(&mut self, raw: Compound) {
        let saved = Self::from_nbt(raw);
        for (key, entry) in &saved.entries {
            self.write_entry(key, Some(entry.clone()));
        }
        let Some(saved_raw) = saved.raw else {
            return;
        };
        let Some(own) = &mut self.raw else {
            self.raw = Some(saved_raw);
            return;
        };
        let own = Arc::make_mut(own);
        for (namespace, tag) in saved_raw.iter() {
            match (own.get_mut(namespace), tag) {
                (Some(Tag::Compound(names)), Tag::Compound(saved_names)) => {
                    for (name, value) in saved_names {
                        names.insert(name.clone(), value.clone());
                    }
                }
                _ => {
                    own.insert(namespace.clone(), tag.clone());
                }
            }
        }
    }
    /// Encode changed values, preserving raw entries and unrecognized tags.
    pub fn to_nbt(&self) -> Compound {
        let mut raw = self.raw.as_deref().cloned().unwrap_or_default();
        for ((namespace, name), entry) in &self.entries {
            let value = match &entry.state {
                Entry::Raw {
                    saved,
                    kind: RawKind::Saved,
                    ..
                } => Some(saved.clone()),
                Entry::Raw {
                    kind: RawKind::Synced,
                    ..
                } => None,
                Entry::Decoded(value) => envelope(value),
            };
            if let Some(value) = value {
                let tag = raw
                    .entry(namespace.clone())
                    .or_insert_with(|| Tag::Compound(Compound::new()));
                let Tag::Compound(names) = tag else {
                    unreachable!("attachment namespace checked before mutation");
                };
                names.insert(name.clone(), Tag::Compound(value));
            }
        }
        raw
    }
    /// Copy opted-in values through their disk encoding for a player respawn.
    pub fn copy_on_death(&self) -> Result<Self, AttachmentError> {
        let mut copy = Self::default();
        for (key, entry) in &self.entries {
            let encoded = match &entry.state {
                Entry::Raw {
                    saved,
                    kind: RawKind::Saved,
                    ..
                } if matches!(saved.get(&"copy_on_death".into()), Some(Tag::Byte(1))) => {
                    Some(saved.clone())
                }
                Entry::Decoded(value) if value.policy.copy_on_death => {
                    Some(envelope(value).ok_or(AttachmentError::InvalidCopyPolicy)?)
                }
                _ => None,
            };
            if let Some(encoded) = encoded {
                copy.entries
                    .insert(key.clone(), Arc::new(AttachmentEntry::raw(encoded)));
            }
        }
        Ok(copy)
    }
    /// The shared immutable binding, without decoding or inserting a value.
    #[doc(hidden)]
    pub fn entry(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<&Arc<AttachmentEntry>>, AttachmentError> {
        self.check_namespace(namespace)?;
        Ok(self
            .entries
            .get(&(namespace, name) as &dyn AttachmentKeyView))
    }
    /// Whether a value is stored, saved or set, without decoding or inserting it.
    fn holds(&self, namespace: &str, name: &str) -> bool {
        self.entries
            .contains_key(&(namespace, name) as &dyn AttachmentKeyView)
    }
    /// Refuse a namespace whose saved form isn't a compound: nothing under it can be read or
    /// written without losing it.
    fn check_namespace(&self, namespace: &str) -> Result<(), AttachmentError> {
        thread_local! {
            static NAMESPACE: std::cell::RefCell<JavaString> = Default::default();
        }
        let invalid = self.raw.as_ref().is_some_and(|raw| {
            NAMESPACE.with_borrow_mut(|key| {
                key.0.clear();
                key.0.extend(namespace.encode_utf16());
                raw.get(key)
                    .is_some_and(|tag| !matches!(tag, Tag::Compound(_)))
            })
        });
        match invalid {
            true => Err(AttachmentError::InvalidNamespace),
            _ => Ok(()),
        }
    }
    fn decoded(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<AttachmentValue>, AttachmentError> {
        Ok(
            match self.entry(namespace, name)?.map(|entry| &entry.state) {
                Some(Entry::Decoded(value)) => Some(value.value.clone()),
                _ => None,
            },
        )
    }
    fn saved(&self, namespace: &str, name: &str) -> Result<Option<Compound>, AttachmentError> {
        match self.entry(namespace, name)?.map(|entry| &entry.state) {
            Some(Entry::Raw {
                saved,
                kind: RawKind::Saved,
                ..
            }) => raw_value(saved)
                .cloned()
                .map(Some)
                .ok_or(AttachmentError::Decode),
            _ => Ok(None),
        }
    }
    fn synced_raw(&self, namespace: &str, name: &str) -> Result<Option<Compound>, AttachmentError> {
        match self.entry(namespace, name)?.map(|entry| &entry.state) {
            Some(Entry::Raw {
                saved,
                kind: RawKind::Synced,
                ..
            }) => raw_value(saved)
                .cloned()
                .map(Some)
                .ok_or(AttachmentError::Decode),
            _ => Ok(None),
        }
    }
    fn store(&mut self, namespace: &str, name: &str, value: Decoded, write: AttachmentWrite) {
        let sync = value.policy.sync;
        // UTF-8 byte length is an upper bound on UTF-16 units. Reserve once
        // instead of repeatedly growing the short key buffers during encoding.
        let [owned_namespace, owned_name] = [namespace, name].map(|text| {
            let mut units = Vec::with_capacity(text.len());
            if text.is_ascii() {
                units.extend(text.bytes().map(u16::from));
            } else {
                units.extend(text.encode_utf16());
            }
            JavaString(units)
        });
        let key = (owned_namespace, owned_name);
        self.write_entry(
            &key,
            Some(Arc::new(AttachmentEntry {
                state: Entry::Decoded(value),
            })),
        );
        if sync && write != AttachmentWrite::Decoded {
            self.pending.insert(key);
        }
        if write == AttachmentWrite::Set {
            self.remove_saved(namespace, name);
        }
    }
    fn discard(&mut self, namespace: &str, name: &str, sync: bool) {
        let key: (JavaString, JavaString) = (namespace.into(), name.into());
        self.write_entry(&key, None);
        if sync {
            self.pending.insert(key);
        }
        self.remove_saved(namespace, name);
    }
    fn remove_saved(&mut self, namespace: &str, name: &str) {
        if let Some(raw) = &mut self.raw
            && let Some(Tag::Compound(names)) = Arc::make_mut(raw).get_mut(&namespace.into())
        {
            names.remove(&name.into());
        }
    }
    fn queue_sync(&mut self, namespace: &str, name: &str) {
        self.pending.insert((namespace.into(), name.into()));
    }
    /// The changes to send to clients since the last call: the current value
    /// of each queued key, or `None` for one that is gone.
    pub fn take_sync_changes(&mut self) -> Vec<AttachmentSync> {
        std::mem::take(&mut self.pending)
            .into_iter()
            .map(|(namespace, name)| {
                let value = self
                    .entries
                    .get(&(namespace.clone(), name.clone()))
                    .and_then(|entry| entry.synced_value())
                    .cloned();
                AttachmentSync {
                    namespace,
                    name,
                    value,
                }
            })
            .collect()
    }
    /// Every synced value, for a client that starts seeing the holder.
    pub fn synced(&self) -> Vec<AttachmentSync> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.is_synced())
            .filter_map(|((namespace, name), entry)| {
                Some(AttachmentSync {
                    namespace: namespace.clone(),
                    name: name.clone(),
                    value: Some(entry.synced_value()?.clone()),
                })
            })
            .collect()
    }
    /// Client side: keep what the server sent, decoded by the next `get`.
    pub fn apply_sync(&mut self, change: AttachmentSync) {
        let key = (change.namespace, change.name);
        let entry = change
            .value
            .map(|value| Arc::new(AttachmentEntry::raw_synced(value)));
        self.write_entry(&key, entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(n: &i32) -> Result<Compound, AttachmentError> {
        Ok([(JavaString::from("value"), Tag::Int(*n))].into())
    }
    fn decode(c: &Compound) -> Result<i32, AttachmentError> {
        match c.get(&"value".into()) {
            Some(Tag::Int(n)) => Ok(*n),
            _ => Err(AttachmentError::Decode),
        }
    }
    static COUNTER: AttachmentType<i32> = AttachmentType {
        namespace: "test",
        name: "counter",
        default: || 0,
        codec: Some(AttachmentCodec { encode, decode }),
        copy_on_death: false,
        sync_codec: None,
    };
    static SYNCED: AttachmentType<i32> = AttachmentType {
        namespace: "test",
        name: "synced",
        default: || 7,
        codec: Some(AttachmentCodec { encode, decode }),
        copy_on_death: false,
        sync_codec: Some(AttachmentCodec { encode, decode }),
    };
    fn change(name: &str, value: Option<i32>) -> AttachmentSync {
        AttachmentSync {
            namespace: "test".into(),
            name: name.into(),
            value: value.map(|n| encode(&n).unwrap()),
        }
    }
    static MEMORY: AttachmentType<i32> = AttachmentType {
        namespace: "test",
        name: "memory",
        default: || 3,
        codec: None,
        copy_on_death: false,
        sync_codec: None,
    };
    fn rows(
        index: &Arc<Mutex<index::AttachmentIndex<u32>>>,
        key: &AttachmentType<i32>,
    ) -> Arc<index::AttachmentSnapshot<u32>> {
        index
            .lock()
            .unwrap()
            .snapshot(&(key.namespace.into(), key.name.into()))
    }

    fn indexed_values(
        index: &Arc<Mutex<index::AttachmentIndex<u32>>>,
        key: &AttachmentType<i32>,
    ) -> Vec<(u32, i32)> {
        rows(index, key)
            .iter()
            .map(|row| (row.holder, *row.entry.read(key).unwrap()))
            .collect()
    }

    #[test]
    fn binding_publishes_unread_saved_entries_and_canonical_decode() {
        let mut source = Attachments::default();
        COUNTER.set(&mut source, Holder::Level, 42).unwrap();
        SYNCED.set(&mut source, Holder::Level, 8).unwrap();
        let saved = source.to_nbt();
        let mut map = Attachments::from_nbt(saved.clone());
        let index = Arc::new(Mutex::new(index::AttachmentIndex::default()));
        map.bind_index(&index, 1);
        let before = rows(&index, &COUNTER);
        assert!(Arc::ptr_eq(
            &before[0].entry,
            map.entry("test", "counter").unwrap().unwrap()
        ));
        assert_eq!(indexed_values(&index, &COUNTER), [(1, 42)]);
        assert_eq!(indexed_values(&index, &SYNCED), [(1, 8)]);
        assert_eq!(map.to_nbt(), saved);
        assert!(map.take_sync_changes().is_empty());
        assert_eq!(COUNTER.get_existing(&mut map, Holder::Level), Ok(Some(42)));
        let decoded = rows(&index, &COUNTER);
        assert!(!Arc::ptr_eq(&before[0].entry, &decoded[0].entry));
        assert!(Arc::ptr_eq(
            &decoded[0].entry,
            map.entry("test", "counter").unwrap().unwrap()
        ));
        assert_eq!(before[0].entry.read(&COUNTER), Ok(&42));
        assert_eq!(map.to_nbt(), saved);
        assert!(map.take_sync_changes().is_empty());
    }

    #[test]
    fn bound_default_set_remove_and_client_sync_publish_current_shared_values() {
        let index = Arc::new(Mutex::new(index::AttachmentIndex::default()));
        let mut map = Attachments::default();
        map.bind_index(&index, 1);
        assert!(rows(&index, &SYNCED).is_empty());
        assert_eq!(SYNCED.get(&mut map, Holder::Level), Ok(7));
        let default = rows(&index, &SYNCED);
        assert_eq!(map.take_sync_changes(), [change("synced", Some(7))]);
        SYNCED.set(&mut map, Holder::Level, 9).unwrap();
        assert_eq!(indexed_values(&index, &SYNCED), [(1, 9)]);
        assert_eq!(SYNCED.remove(&mut map, Holder::Level), Ok(Some(9)));
        assert!(rows(&index, &SYNCED).is_empty());
        assert_eq!(map.take_sync_changes(), [change("synced", None)]);
        map.apply_sync(change("synced", Some(12)));
        assert_eq!(indexed_values(&index, &SYNCED), [(1, 12)]);
        assert!(map.take_sync_changes().is_empty());
        map.apply_sync(change("synced", None));
        assert!(rows(&index, &SYNCED).is_empty());
        assert_eq!(default[0].entry.read(&SYNCED), Ok(&7));
    }

    #[test]
    fn clone_is_detached_moves_keep_publication_and_drop_never_unloads() {
        let index = Arc::new(Mutex::new(index::AttachmentIndex::default()));
        let mut map = Attachments::default();
        COUNTER.set(&mut map, Holder::Level, 1).unwrap();
        let registration = map.bind_index(&index, 1);
        let mut detached = map.clone();
        COUNTER.set(&mut detached, Holder::Level, 2).unwrap();
        COUNTER.remove(&mut detached, Holder::Level).unwrap();
        detached.apply_sync(change("counter", Some(3)));
        drop(detached);
        drop(registration.clone());
        assert_eq!(indexed_values(&index, &COUNTER), [(1, 1)]);
        let mut moved = map;
        COUNTER.set(&mut moved, Holder::Level, 4).unwrap();
        drop(moved);
        assert_eq!(indexed_values(&index, &COUNTER), [(1, 4)]);
        assert!(registration.unregister());
        assert!(rows(&index, &COUNTER).is_empty());
    }

    #[test]
    fn cow_rebind_keeps_entries_and_rejects_stale_rows_maps_and_lifetime_handles() {
        let index = Arc::new(Mutex::new(index::AttachmentIndex::default()));
        let mut old = Attachments::default();
        COUNTER.set(&mut old, Holder::Level, 1).unwrap();
        let old_registration = old.bind_index(&index, 1);
        let mut neighbor = old.clone();
        COUNTER.set(&mut neighbor, Holder::Level, 2).unwrap();
        neighbor.bind_index(&index, 2);
        let before = rows(&index, &COUNTER);
        let mut current = old.clone();
        let registration = current.bind_index(&index, 1);
        assert!(Arc::ptr_eq(
            &before[0].entry,
            &rows(&index, &COUNTER)[0].entry
        ));
        assert_eq!(
            index.lock().unwrap().validate_row(&before[0]),
            Err(AttachmentRowError::StaleHolder)
        );
        COUNTER.set(&mut old, Holder::Level, 99).unwrap();
        COUNTER.remove(&mut old, Holder::Level).unwrap();
        old.apply_sync(change("counter", Some(100)));
        old.replace_contents(Attachments::default());
        assert!(!old_registration.unregister());
        assert_eq!(indexed_values(&index, &COUNTER), [(1, 1), (2, 2)]);
        COUNTER.set(&mut current, Holder::Level, 3).unwrap();
        assert_eq!(indexed_values(&index, &COUNTER), [(1, 3), (2, 2)]);
        assert_eq!(before[0].entry.read(&COUNTER), Ok(&1));
        assert!(registration.unregister());
        assert!(!registration.unregister());
        COUNTER.set(&mut current, Holder::Level, 4).unwrap();
        assert_eq!(indexed_values(&index, &COUNTER), [(2, 2)]);
        assert_eq!(COUNTER.get(&mut current, Holder::Level), Ok(4));
    }

    #[test]
    fn restoring_contents_restores_membership_payloads_saved_data_and_future_writes() {
        let index = Arc::new(Mutex::new(index::AttachmentIndex::default()));
        let mut map = Attachments::from_nbt([("unknown".into(), Tag::Int(5))].into());
        SYNCED.set(&mut map, Holder::Level, 10).unwrap();
        MEMORY.set(&mut map, Holder::Level, 20).unwrap();
        map.bind_index(&index, 1);
        let kept = map.clone();
        let saved = kept.to_nbt();
        SYNCED.set(&mut map, Holder::Level, 11).unwrap();
        MEMORY.remove(&mut map, Holder::Level).unwrap();
        COUNTER.set(&mut map, Holder::Level, 30).unwrap();
        let aborted = rows(&index, &SYNCED);
        map.replace_contents(kept);
        assert_eq!(map.to_nbt(), saved);
        assert_eq!(map.take_sync_changes(), [change("synced", Some(10))]);
        assert_eq!(indexed_values(&index, &SYNCED), [(1, 10)]);
        assert_eq!(indexed_values(&index, &MEMORY), [(1, 20)]);
        assert!(rows(&index, &COUNTER).is_empty());
        assert_eq!(aborted[0].entry.read(&SYNCED), Ok(&11));
        SYNCED.set(&mut map, Holder::Level, 12).unwrap();
        assert_eq!(indexed_values(&index, &SYNCED), [(1, 12)]);
        map.replace_contents(Attachments::default());
        assert!(rows(&index, &SYNCED).is_empty());
        assert!(rows(&index, &MEMORY).is_empty());
        COUNTER.get(&mut map, Holder::Level).unwrap();
        assert_eq!(indexed_values(&index, &COUNTER), [(1, 0)]);
    }

    #[test]
    fn replacing_contents_keeps_only_the_receivers_registration_and_unchanged_rows() {
        let index = Arc::new(Mutex::new(index::AttachmentIndex::default()));
        let mut map = Attachments::default();
        COUNTER.set(&mut map, Holder::Level, 1).unwrap();
        let registration = map.bind_index(&index, 1);
        let before = rows(&index, &COUNTER);
        map.replace_contents(map.clone());
        assert!(Arc::ptr_eq(&before, &rows(&index, &COUNTER)));
        let mut other = map.clone();
        COUNTER.set(&mut other, Holder::Level, 2).unwrap();
        let other_registration = other.bind_index(&index, 2);
        map.replace_contents(other);
        COUNTER.set(&mut map, Holder::Level, 3).unwrap();
        assert_eq!(indexed_values(&index, &COUNTER), [(1, 3), (2, 2)]);
        assert!(registration.unregister());
        assert_eq!(indexed_values(&index, &COUNTER), [(2, 2)]);
        assert!(other_registration.unregister());
    }

    #[test]
    fn failed_typed_removal_still_publishes_removal_of_the_unread_saved_key() {
        let raw = [(
            "test".into(),
            Tag::Compound([("counter".into(), Tag::Compound(Compound::new()))].into()),
        )]
        .into();
        let mut map = Attachments::from_nbt(raw);
        let index = Arc::new(Mutex::new(index::AttachmentIndex::default()));
        map.bind_index(&index, 1);
        let before = rows(&index, &COUNTER);
        assert_eq!(before.len(), 1);
        assert_eq!(
            COUNTER.remove(&mut map, Holder::Level),
            Err(AttachmentError::Decode)
        );
        assert!(rows(&index, &COUNTER).is_empty());
        assert_eq!(before[0].entry.read(&COUNTER), Err(AttachmentError::Decode));
    }

    #[test]
    fn expired_index_does_not_keep_maps_or_registration_from_standalone_use() {
        let index = Arc::new(Mutex::new(index::AttachmentIndex::default()));
        let weak = Arc::downgrade(&index);
        let mut map = Attachments::default();
        let registration = map.bind_index(&index, 1);
        drop(index);
        assert!(weak.upgrade().is_none());
        assert_eq!(SYNCED.get(&mut map, Holder::Level), Ok(7));
        SYNCED.set(&mut map, Holder::Level, 8).unwrap();
        let saved = map.clone();
        SYNCED.remove(&mut map, Holder::Level).unwrap();
        map.replace_contents(saved);
        assert_eq!(SYNCED.get(&mut map, Holder::Level), Ok(8));
        map.apply_sync(change("synced", Some(9)));
        assert_eq!(SYNCED.get(&mut map, Holder::Level), Ok(9));
        assert!(!registration.unregister());
    }

    #[test]
    fn retirement_and_rebind_drop_mod_payloads_only_after_unlock() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        #[derive(Clone)]
        struct Probe {
            index: Weak<Mutex<index::AttachmentIndex<u32>>>,
            unlocked: Arc<AtomicBool>,
            drops: Arc<AtomicUsize>,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                if let Some(index) = self.index.upgrade() {
                    self.unlocked
                        .fetch_and(index.try_lock().is_ok(), Ordering::Relaxed);
                }
                self.drops.fetch_add(1, Ordering::Relaxed);
            }
        }
        let key: AttachmentType<Probe> = AttachmentType {
            namespace: "test",
            name: "drop_probe",
            default: || panic!("no default"),
            codec: None,
            copy_on_death: false,
            sync_codec: None,
        };
        let mut outcomes = Vec::new();
        for rebind in [false, true] {
            let index = Arc::new(Mutex::new(index::AttachmentIndex::default()));
            let unlocked = Arc::new(AtomicBool::new(true));
            let drops = Arc::new(AtomicUsize::new(0));
            let mut map = Attachments::default();
            key.set(
                &mut map,
                Holder::Level,
                Probe {
                    index: Arc::downgrade(&index),
                    unlocked: unlocked.clone(),
                    drops: drops.clone(),
                },
            )
            .unwrap();
            let registration = map.bind_index(&index, 1);
            let mut replacement = map.clone();
            key.remove(&mut replacement, Holder::Level).unwrap();
            drop(map);
            if rebind {
                replacement.bind_index(&index, 1);
            } else {
                assert!(registration.unregister());
            }
            assert_eq!(drops.load(Ordering::Relaxed), 2);
            outcomes.push(unlocked.load(Ordering::Relaxed));
        }
        assert_eq!(outcomes, [true, true]);
    }

    #[test]
    fn defaults_and_codecs_run_without_an_index_lock() {
        static INDEX: OnceLock<Arc<Mutex<index::AttachmentIndex<u32>>>> = OnceLock::new();
        fn probe() {
            drop(
                INDEX
                    .get()
                    .unwrap()
                    .try_lock()
                    .expect("mod code under index lock"),
            );
        }
        let key = AttachmentType {
            namespace: "test",
            name: "probe",
            default: || {
                probe();
                5
            },
            codec: Some(AttachmentCodec {
                encode: |n| {
                    probe();
                    encode(n)
                },
                decode: |form| {
                    probe();
                    decode(form)
                },
            }),
            copy_on_death: false,
            sync_codec: None,
        };
        let index = INDEX.get_or_init(Default::default);
        let mut map = Attachments::default();
        map.bind_index(index, 1);
        assert_eq!(key.get(&mut map, Holder::Level), Ok(5));
        key.set(&mut map, Holder::Level, 6).unwrap();
        let mut loaded = Attachments::from_nbt(map.to_nbt());
        loaded.bind_index(index, 1);
        assert_eq!(indexed_values(index, &key), [(1, 6)]);
        assert_eq!(key.get_existing(&mut loaded, Holder::Level), Ok(Some(6)));
    }

    #[test]
    fn saved_values_read_over_a_map_replace_only_their_own_keys() {
        let mut source = Attachments::from_nbt([("unknown".into(), Tag::Int(5))].into());
        COUNTER.set(&mut source, Holder::Level, 42).unwrap();
        let saved = source.to_nbt();
        // What the constructing listeners set before the load.
        let mut map = Attachments::default();
        COUNTER.set(&mut map, Holder::Level, 1).unwrap();
        SYNCED.set(&mut map, Holder::Level, 9).unwrap();
        map.read_saved_over(saved);
        assert_eq!(COUNTER.get(&mut map, Holder::Level), Ok(42));
        assert_eq!(SYNCED.get(&mut map, Holder::Level), Ok(9));
        let mut reloaded = Attachments::from_nbt(map.to_nbt());
        assert_eq!(COUNTER.get(&mut reloaded, Holder::Level), Ok(42));
        assert_eq!(SYNCED.get(&mut reloaded, Holder::Level), Ok(9));
        assert_eq!(reloaded.to_nbt().get(&"unknown".into()), Some(&Tag::Int(5)));

        // A map with saved data of its own keeps it beside what is read over it.
        let mut own = Attachments::from_nbt([("kept".into(), Tag::Int(1))].into());
        own.read_saved_over([("unknown".into(), Tag::Int(5))].into());
        let nbt = own.to_nbt();
        assert_eq!(nbt.get(&"kept".into()), Some(&Tag::Int(1)));
        assert_eq!(nbt.get(&"unknown".into()), Some(&Tag::Int(5)));
    }

    #[test]
    fn empty_and_new_maps_need_no_saved_backing_allocation() {
        let mut map = Attachments::default();
        assert!(map.raw.is_none());
        COUNTER.set(&mut map, Holder::Level, 1).unwrap();
        assert!(map.raw.is_none());
        COUNTER.remove(&mut map, Holder::Level).unwrap();
        assert!(map.raw.is_none());
        assert!(Attachments::from_nbt(Compound::new()).raw.is_none());
    }
    #[test]
    fn shared_bindings_and_map_snapshots_keep_replaced_and_removed_values() {
        let mut map = Attachments::default();
        SYNCED.set(&mut map, Holder::Level, 1).unwrap();
        map.take_sync_changes();
        let loaded = Attachments::from_nbt(map.to_nbt());
        for mut map in [map, loaded] {
            let mut snapshot = map.clone();
            let entry = map.entry("test", "synced").unwrap().unwrap().clone();
            assert!(Arc::ptr_eq(
                &entry,
                snapshot.entry("test", "synced").unwrap().unwrap()
            ));
            let saved = snapshot.to_nbt();

            SYNCED.set(&mut map, Holder::Level, 2).unwrap();
            assert_eq!(entry.read(&SYNCED), Ok(&1));
            assert_eq!(SYNCED.get_existing(&mut map, Holder::Level), Ok(Some(2)));
            assert_eq!(SYNCED.remove(&mut map, Holder::Level), Ok(Some(2)));
            assert!(map.entry("test", "synced").unwrap().is_none());
            assert_eq!(entry.read(&SYNCED), Ok(&1));
            assert_eq!(
                SYNCED.get_existing(&mut snapshot, Holder::Level),
                Ok(Some(1))
            );
            assert_eq!(snapshot.to_nbt(), saved);
            assert!(snapshot.take_sync_changes().is_empty());
            assert_eq!(map.take_sync_changes(), [change("synced", None)]);
        }
    }
    #[test]
    fn raw_read_cache_borrows_without_claiming_the_canonical_type_or_policy() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static DECODES: AtomicUsize = AtomicUsize::new(0);
        struct NonClone(i32);
        let key = AttachmentType {
            namespace: "test",
            name: "counter",
            default: || panic!("a query must not create a default"),
            codec: Some(AttachmentCodec {
                encode: |_: &NonClone| panic!("a query must not encode"),
                decode: |saved| {
                    DECODES.fetch_add(1, Ordering::Relaxed);
                    decode(saved).map(NonClone)
                },
            }),
            copy_on_death: true,
            sync_codec: Some(AttachmentCodec {
                encode: |_: &NonClone| panic!("a query must not encode"),
                decode: |_: &Compound| panic!("a disk read must use persistence codec"),
            }),
        };
        let mut original = Attachments::default();
        COUNTER.set(&mut original, Holder::Level, 42).unwrap();
        let saved = original.to_nbt();
        let mut map = Attachments::from_nbt(saved.clone());
        let snapshot = map.clone();
        assert!(Arc::ptr_eq(
            map.raw.as_ref().unwrap(),
            snapshot.raw.as_ref().unwrap()
        ));
        let entry = map.entry("test", "counter").unwrap().unwrap().clone();
        let value = entry.read(&key).unwrap();
        assert_eq!(value.0, 42);
        assert!(std::ptr::eq(value, entry.read(&key).unwrap()));
        assert!(std::ptr::eq(
            value,
            snapshot
                .entry("test", "counter")
                .unwrap()
                .unwrap()
                .read(&key)
                .unwrap()
        ));
        assert_eq!(DECODES.load(Ordering::Relaxed), 1);
        assert!(map.decoded("test", "counter").unwrap().is_none());
        assert_eq!(map.to_nbt(), saved);
        assert!(map.synced().is_empty());
        assert!(map.take_sync_changes().is_empty());
        assert!(
            map.copy_on_death()
                .unwrap()
                .entry("test", "counter")
                .unwrap()
                .is_none()
        );

        // Another query type gets its own borrowed cache; ordinary reads still decode raw.
        assert_eq!(entry.read(&COUNTER), Ok(&42));
        assert!(std::ptr::eq(value, entry.read(&key).unwrap()));
        assert_eq!(COUNTER.get_existing(&mut map, Holder::Level), Ok(Some(42)));
        let decoded = map.entry("test", "counter").unwrap().unwrap();
        assert!(matches!(
            decoded.read(&key),
            Err(AttachmentError::TypeMismatch)
        ));
        assert_eq!(map.to_nbt(), saved);
        assert!(map.take_sync_changes().is_empty());
        assert_eq!(entry.read(&key).unwrap().0, 42);
    }
    #[test]
    fn query_reads_reject_invalid_policy_without_populating_the_cache() {
        let mut map = Attachments::default();
        map.apply_sync(change("synced", Some(9)));
        let invalid = AttachmentType {
            codec: None,
            copy_on_death: true,
            ..SYNCED
        };
        let entry = map.entry("test", "synced").unwrap().unwrap();
        assert_eq!(
            entry.read(&invalid),
            Err(AttachmentError::InvalidCopyPolicy)
        );
        assert_eq!(entry.read(&SYNCED), Ok(&9));
        assert!(map.take_sync_changes().is_empty());
    }
    #[test]
    fn failed_query_decodes_are_retried_without_changing_the_saved_binding() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
        let key = AttachmentType {
            sync_codec: Some(AttachmentCodec {
                encode,
                decode: |saved| {
                    if ATTEMPTS.fetch_add(1, Ordering::Relaxed) == 0 {
                        Err(AttachmentError::Decode)
                    } else {
                        decode(saved)
                    }
                },
            }),
            ..SYNCED
        };
        let mut map = Attachments::default();
        map.apply_sync(change("synced", Some(9)));
        let saved = map.to_nbt();
        let entry = map.entry("test", "synced").unwrap().unwrap();
        assert_eq!(entry.read(&key), Err(AttachmentError::Decode));
        assert_eq!(entry.read(&key), Ok(&9));
        assert_eq!(entry.read(&key), Ok(&9));
        assert_eq!(ATTEMPTS.load(Ordering::Relaxed), 2);
        assert!(map.decoded("test", "synced").unwrap().is_none());
        assert_eq!(map.to_nbt(), saved);
        assert!(map.take_sync_changes().is_empty());
    }
    #[test]
    fn sync_replacement_and_removal_leave_the_shared_old_entry_readable() {
        let mut map = Attachments::default();
        map.apply_sync(change("synced", Some(9)));
        let old = map.entry("test", "synced").unwrap().unwrap().clone();
        map.apply_sync(change("synced", Some(10)));
        assert_eq!(old.read(&SYNCED), Ok(&9));
        assert_eq!(SYNCED.get_existing(&mut map, Holder::Level), Ok(Some(10)));
        map.apply_sync(change("synced", None));
        assert_eq!(old.read(&SYNCED), Ok(&9));
        assert_eq!(SYNCED.get_existing(&mut map, Holder::Level), Ok(None));
    }
    #[test]
    fn saved_values_round_trip_and_memory_values_do_not() {
        let mut map = Attachments::default();
        COUNTER.set(&mut map, Holder::Level, 42).unwrap();
        MEMORY.set(&mut map, Holder::Level, 8).unwrap();
        let mut loaded = Attachments::from_nbt(map.to_nbt());
        assert_eq!(COUNTER.get(&mut loaded, Holder::Level), Ok(42));
        assert_eq!(MEMORY.get(&mut loaded, Holder::Level), Ok(3));
        assert_eq!(COUNTER.remove(&mut loaded, Holder::Level), Ok(Some(42)));
        let mut reopened = Attachments::from_nbt(loaded.to_nbt());
        assert_eq!(COUNTER.get(&mut reopened, Holder::Level), Ok(0));
    }
    #[test]
    fn untouched_saved_value_copies_on_respawn() {
        let keep = AttachmentType {
            namespace: "test",
            name: "keep",
            default: || 0,
            codec: Some(AttachmentCodec { encode, decode }),
            copy_on_death: true,
            sync_codec: None,
        };
        let mut original = Attachments::default();
        keep.set(&mut original, Holder::Level, 42).unwrap();
        COUNTER.set(&mut original, Holder::Level, 5).unwrap();
        let loaded = Attachments::from_nbt(original.to_nbt());
        let mut respawned = loaded.copy_on_death().unwrap();
        assert_eq!(keep.get(&mut respawned, Holder::Level), Ok(42));
        assert_eq!(COUNTER.get(&mut respawned, Holder::Level), Ok(0));
    }
    #[test]
    fn existing_reads_leave_an_absent_value_absent() {
        let mut map = Attachments::default();
        assert!(!COUNTER.has(&mut map, Holder::Level).unwrap());
        assert_eq!(COUNTER.get_existing(&mut map, Holder::Level), Ok(None));
        assert!(!COUNTER.has(&mut map, Holder::Level).unwrap());
        assert_eq!(map.to_nbt(), Compound::new());
        assert_eq!(COUNTER.get(&mut map, Holder::Level), Ok(0));
        assert!(COUNTER.has(&mut map, Holder::Level).unwrap());
        assert_ne!(map.to_nbt(), Compound::new());
        COUNTER.set(&mut map, Holder::Level, 5).unwrap();
        assert_eq!(COUNTER.get_existing(&mut map, Holder::Level), Ok(Some(5)));
    }
    #[test]
    fn existing_read_decodes_a_saved_value_without_changing_it() {
        let mut map = Attachments::default();
        COUNTER.set(&mut map, Holder::Level, 42).unwrap();
        MEMORY.set(&mut map, Holder::Level, 8).unwrap();
        let saved = map.to_nbt();
        let mut loaded = Attachments::from_nbt(saved.clone());
        assert!(COUNTER.has(&mut loaded, Holder::Level).unwrap());
        assert_eq!(
            COUNTER.get_existing(&mut loaded, Holder::Level),
            Ok(Some(42))
        );
        assert_eq!(loaded.to_nbt(), saved);
        assert!(!MEMORY.has(&mut loaded, Holder::Level).unwrap());
        assert_eq!(MEMORY.get_existing(&mut loaded, Holder::Level), Ok(None));
    }
    #[test]
    fn unknown_and_invalid_values_survive_a_failed_decode() {
        let saved: Compound = [(
            JavaString::from("test"),
            Tag::Compound(
                [
                    (
                        JavaString::from("counter"),
                        Tag::Compound(
                            [(JavaString::from("value"), Tag::String("bad".into()))].into(),
                        ),
                    ),
                    (JavaString(vec![0xd800]), Tag::Compound(Compound::new())),
                ]
                .into(),
            ),
        )]
        .into();
        let mut map = Attachments::from_nbt(saved.clone());
        let old = map.entry("test", "counter").unwrap().unwrap().clone();
        assert_eq!(old.read(&COUNTER), Err(AttachmentError::Decode));
        assert!(COUNTER.has(&mut map, Holder::Level).unwrap());
        assert_eq!(
            COUNTER.get_existing(&mut map, Holder::Level),
            Err(AttachmentError::Decode)
        );
        assert_eq!(
            COUNTER.get(&mut map, Holder::Level),
            Err(AttachmentError::Decode)
        );
        assert_eq!(map.to_nbt(), saved);
        assert_eq!(
            COUNTER.remove(&mut map, Holder::Level),
            Err(AttachmentError::Decode)
        );
        assert_eq!(
            COUNTER.remove(&mut map, Holder::Level),
            Ok(None),
            "removed all the same"
        );
        assert_eq!(old.read(&COUNTER), Err(AttachmentError::Decode));
    }
    #[test]
    fn synced_set_and_remove_report_the_last_state_once() {
        let mut map = Attachments::default();
        SYNCED.set(&mut map, Holder::Level, 1).unwrap();
        SYNCED.set(&mut map, Holder::Level, 2).unwrap();
        SYNCED.remove(&mut map, Holder::Level).unwrap();
        assert_eq!(map.take_sync_changes(), [change("synced", None)]);
        assert!(map.take_sync_changes().is_empty());
    }
    #[test]
    fn default_creation_is_synced_and_a_later_read_is_not() {
        let mut map = Attachments::default();
        assert_eq!(SYNCED.get(&mut map, Holder::Level), Ok(7));
        assert_eq!(map.take_sync_changes(), [change("synced", Some(7))]);
        assert_eq!(SYNCED.get(&mut map, Holder::Level), Ok(7));
        assert!(map.take_sync_changes().is_empty());
    }
    #[test]
    fn unsynced_types_report_no_changes() {
        let mut map = Attachments::default();
        COUNTER.set(&mut map, Holder::Level, 1).unwrap();
        COUNTER.remove(&mut map, Holder::Level).unwrap();
        COUNTER.sync_data(&mut map, Holder::Level).unwrap();
        assert!(map.take_sync_changes().is_empty());
    }
    #[test]
    fn sync_without_persistence_codec_syncs_and_does_not_persist() {
        let sync_only = AttachmentType {
            namespace: "test",
            name: "ephemeral",
            default: || 5,
            codec: None,
            copy_on_death: false,
            sync_codec: Some(AttachmentCodec { encode, decode }),
        };
        let mut server = Attachments::default();
        assert_eq!(sync_only.get(&mut server, Holder::Level), Ok(5));
        assert_eq!(server.take_sync_changes(), [change("ephemeral", Some(5))]);
        sync_only.set(&mut server, Holder::Level, 11).unwrap();
        assert_eq!(server.to_nbt(), Compound::new());
        assert_eq!(server.synced(), [change("ephemeral", Some(11))]);
        let updates = server.take_sync_changes();
        assert_eq!(updates, [change("ephemeral", Some(11))]);

        let mut client = Attachments::default();
        for update in updates {
            client.apply_sync(update);
        }
        assert_eq!(client.to_nbt(), Compound::new());
        assert_eq!(
            client
                .entry("test", "ephemeral")
                .unwrap()
                .unwrap()
                .read(&sync_only),
            Ok(&11)
        );
        assert_eq!(
            sync_only.get_existing(&mut client, Holder::Level),
            Ok(Some(11))
        );
        assert_eq!(client.to_nbt(), Compound::new());

        // copy_on_death still requires a persistence codec even when sync_codec is present.
        let bad_copy = AttachmentType {
            copy_on_death: true,
            ..sync_only
        };
        assert_eq!(
            bad_copy.set(&mut server, Holder::Level, 1),
            Err(AttachmentError::InvalidCopyPolicy)
        );
    }
    #[test]
    fn distinct_persistence_and_sync_codecs_use_their_own_forms() {
        fn encode_disk(n: &i32) -> Result<Compound, AttachmentError> {
            Ok([(JavaString::from("disk"), Tag::Int(*n))].into())
        }
        fn decode_disk(c: &Compound) -> Result<i32, AttachmentError> {
            match c.get(&"disk".into()) {
                Some(Tag::Int(n)) => Ok(*n),
                _ => Err(AttachmentError::Decode),
            }
        }
        fn encode_net(n: &i32) -> Result<Compound, AttachmentError> {
            Ok([(JavaString::from("net"), Tag::Int(*n + 100))].into())
        }
        fn decode_net(c: &Compound) -> Result<i32, AttachmentError> {
            match c.get(&"net".into()) {
                Some(Tag::Int(n)) => Ok(*n - 100),
                _ => Err(AttachmentError::Decode),
            }
        }
        let dual = AttachmentType {
            namespace: "test",
            name: "dual",
            default: || 0,
            codec: Some(AttachmentCodec {
                encode: encode_disk,
                decode: decode_disk,
            }),
            copy_on_death: false,
            sync_codec: Some(AttachmentCodec {
                encode: encode_net,
                decode: decode_net,
            }),
        };
        let mut server = Attachments::default();
        dual.set(&mut server, Holder::Level, 7).unwrap();
        let expected_sync = AttachmentSync {
            namespace: "test".into(),
            name: "dual".into(),
            value: Some(encode_net(&7).unwrap()),
        };
        assert_eq!(server.take_sync_changes(), [expected_sync.clone()]);
        assert_eq!(server.synced(), [expected_sync.clone()]);

        // Saved disk form uses persistence codec and retains the sync form for unread snapshots.
        let saved = server.to_nbt();
        let Some(Tag::Compound(names)) = saved.get(&"test".into()) else {
            panic!("namespace saved");
        };
        let Some(Tag::Compound(envelope)) = names.get(&"dual".into()) else {
            panic!("entry saved");
        };
        assert_eq!(
            envelope.get(&"value".into()),
            Some(&Tag::Compound(encode_disk(&7).unwrap()))
        );
        let mut reloaded = Attachments::from_nbt(saved.clone());
        assert_eq!(reloaded.synced(), [expected_sync.clone()]);
        assert_eq!(
            reloaded.entry("test", "dual").unwrap().unwrap().read(&dual),
            Ok(&7)
        );
        assert_eq!(dual.get_existing(&mut reloaded, Holder::Level), Ok(Some(7)));
        assert_eq!(reloaded.to_nbt(), saved);

        // Client decodes synced packets with sync_codec without touching the persistence codec.
        let client_view = AttachmentType {
            codec: Some(AttachmentCodec {
                encode: |_| panic!("client sync read must not invoke persistence encode"),
                decode: |_| panic!("client sync read must not invoke persistence decode"),
            }),
            ..dual
        };
        let mut client = Attachments::default();
        client.apply_sync(expected_sync);
        assert_eq!(
            client
                .entry("test", "dual")
                .unwrap()
                .unwrap()
                .read(&client_view),
            Ok(&7)
        );
        assert_eq!(
            client_view.get_existing(&mut client, Holder::Level),
            Ok(Some(7))
        );
        assert_eq!(client.to_nbt(), Compound::new());
    }
    #[test]
    fn a_saved_sync_flag_puts_undecoded_values_in_the_snapshot() {
        let mut map = Attachments::default();
        SYNCED.set(&mut map, Holder::Level, 5).unwrap();
        COUNTER.set(&mut map, Holder::Level, 6).unwrap();
        let saved = map.to_nbt();
        let Some(Tag::Compound(names)) = saved.get(&"test".into()) else {
            panic!("namespace saved");
        };
        let Some(Tag::Compound(envelope)) = names.get(&"synced".into()) else {
            panic!("entry saved");
        };
        assert_eq!(envelope.get(&"sync".into()), Some(&Tag::Byte(1)));
        let loaded = Attachments::from_nbt(saved);
        assert_eq!(loaded.synced(), [change("synced", Some(5))]);
    }
    #[test]
    fn applied_changes_decode_through_the_codec_and_removals_clear() {
        let mut map = Attachments::default();
        map.apply_sync(change("synced", Some(9)));
        assert_eq!(SYNCED.get(&mut map, Holder::Level), Ok(9));
        map.apply_sync(change("synced", None));
        assert_eq!(SYNCED.get(&mut map, Holder::Level), Ok(7));
    }
    #[test]
    fn original_player_reads_never_store_defaults_or_decoded_values() {
        struct One(Attachments);
        impl AttachmentHost for One {
            fn validate_attachment(
                &self,
                namespace: &str,
                name: &str,
                sync: bool,
            ) -> Result<(), AttachmentError> {
                let _ = (namespace, name, sync);
                Ok(())
            }
            fn attachments(
                &mut self,
                _: Holder,
                _: AttachmentAccess,
            ) -> Result<&mut Attachments, AttachmentError> {
                Ok(&mut self.0)
            }
        }
        let mut host = One(Attachments::default());
        let holder = Holder::OriginalPlayer(1);
        assert_eq!(COUNTER.get(&mut host, holder), Ok(0));
        assert_eq!(COUNTER.has(&mut host, holder), Ok(false));
        assert_eq!(COUNTER.get_existing(&mut host, holder), Ok(None));
        for write in [
            AttachmentWrite::Set,
            AttachmentWrite::Default,
            AttachmentWrite::Decoded,
        ] {
            assert_eq!(
                AttachmentStore::store(
                    &mut host,
                    holder,
                    "test",
                    "counter",
                    Arc::new(9i32),
                    AttachmentForm {
                        encoded: None,
                        synced: None,
                        policy: COUNTER.policy(),
                        write,
                    }
                ),
                Err(AttachmentError::MissingHolder),
            );
        }
        let mut saved = Attachments::default();
        COUNTER.set(&mut saved, Holder::Level, 42).unwrap();
        host.0.read_saved_over(saved.to_nbt());
        assert_eq!(COUNTER.get_existing(&mut host, holder), Ok(Some(42)));
        assert_eq!(COUNTER.get_existing(&mut host, holder), Ok(Some(42)));
        host.0.apply_sync(change("synced", Some(9)));
        assert_eq!(SYNCED.get_existing(&mut host, holder), Ok(Some(9)));
        assert_eq!(SYNCED.get_existing(&mut host, holder), Ok(Some(9)));
    }

    #[test]
    fn sync_data_reports_the_current_value() {
        struct One(Attachments);
        impl AttachmentHost for One {
            fn validate_attachment(
                &self,
                namespace: &str,
                name: &str,
                sync: bool,
            ) -> Result<(), AttachmentError> {
                let _ = (namespace, name, sync);
                Ok(())
            }
            fn attachments(
                &mut self,
                _: Holder,
                _: AttachmentAccess,
            ) -> Result<&mut Attachments, AttachmentError> {
                Ok(&mut self.0)
            }
        }
        let mut host = One(Attachments::default());
        SYNCED.set(&mut host, Holder::Level, 4).unwrap();
        host.0.take_sync_changes();
        SYNCED.sync_data(&mut host, Holder::Level).unwrap();
        assert_eq!(host.0.take_sync_changes(), [change("synced", Some(4))]);
    }

    #[test]
    fn checked_attachment_type_validates_copy_policy_and_allows_sync_only() {
        assert_eq!(
            CheckedAttachmentType::new(AttachmentType {
                namespace: "test",
                name: "x",
                default: || 0i32,
                codec: None,
                copy_on_death: true,
                sync_codec: None,
            })
            .err(),
            Some(AttachmentError::InvalidCopyPolicy)
        );
        let sync_only = CheckedAttachmentType::new(AttachmentType {
            namespace: "test",
            name: "sync_only",
            default: || 7i32,
            codec: None,
            copy_on_death: false,
            sync_codec: Some(AttachmentCodec { encode, decode }),
        })
        .expect("sync_codec without persistence codec is valid");
        assert!(!sync_only.has_codec());
        assert!(!sync_only.is_persistent());
        assert!(sync_only.is_synced());
        assert_eq!(
            sync_only.policy(),
            AttachmentPolicy {
                copy_on_death: false,
                sync: true,
            }
        );

        let mut store = Attachments::default();
        assert_eq!(sync_only.get(&mut store, Holder::Level), Ok(7));
        assert_eq!(sync_only.has(&mut store, Holder::Level), Ok(true));
        sync_only.set(&mut store, Holder::Level, 11).unwrap();
        assert_eq!(
            sync_only.get_existing(&mut store, Holder::Level),
            Ok(Some(11))
        );
        assert_eq!(store.take_sync_changes(), [change("sync_only", Some(11))]);
        assert!(store.to_nbt().is_empty());
    }
}

#[cfg(test)]
mod borrowed_key_tests {
    use super::*;
    #[test]
    fn borrowed_order_matches_java_units() {
        let samples = [
            "", "a", "aa", "z", "é", "\u{d7ff}", "\u{e000}", "\u{ffff}", "😀", "𐀀", "a😀",
        ];
        for a in samples {
            for b in samples {
                for c in samples {
                    for d in samples {
                        let owned: (JavaString, JavaString) = (a.into(), b.into());
                        let other: (JavaString, JavaString) = (c.into(), d.into());
                        let borrowed = (a, b);
                        let right = (c, d);
                        assert_eq!(
                            (&borrowed as &dyn AttachmentKeyView).cmp(&right),
                            owned.cmp(&other)
                        );
                        assert_eq!(
                            (&owned as &dyn AttachmentKeyView).cmp(&right),
                            owned.cmp(&other)
                        );
                    }
                }
            }
        }
        // Saved Java names can contain unpaired surrogates as well.
        let malformed = (JavaString(vec![0xd800]), JavaString(vec![0xdc00]));
        let valid = ("𐀀", "");
        assert_eq!(
            (&malformed as &dyn AttachmentKeyView).cmp(&valid),
            malformed.cmp(&(valid.0.into(), valid.1.into()))
        );
    }
}
