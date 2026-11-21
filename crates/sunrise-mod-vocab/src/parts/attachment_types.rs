//! Load-owned typed attachment descriptors, issued keys, and registration traits.
use super::*;
use crate::{
    AttachmentCodec, AttachmentError, AttachmentPolicy, Compound, Holder, Identifier,
    LevelRandomHost,
};
use std::fmt;
use sunrise_mod_runtime::attachments::{
    AttachmentSyncInput, AttachmentType, CheckedAttachmentType,
};

/// Builder for a typed attachment registered during [`Hooks::register`](crate::Hooks::register).
///
/// Mirrors NeoForge's `AttachmentType.Builder`: an attachment is declared once with its
/// namespaced ID (`"namespace:name"`) and default value constructor, then optionally configured
/// with a persistence codec ([`serialize`](Self::serialize)), a client-sync codec
/// ([`sync`](Self::sync)), and [`copy_on_death`](Self::copy_on_death). Persistence and sync
/// codecs are independent.
// no wire: host-local registration builder with callback pointers; never crosses the wire.
pub struct AttachmentDescriptor<T: 'static> {
    id: &'static str,
    default: fn() -> T,
    codec: Option<AttachmentCodec<T>>,
    copy_on_death: bool,
    sync_codec: Option<AttachmentCodec<T>>,
}

impl<T: 'static> AttachmentDescriptor<T> {
    /// Start a descriptor for the namespaced attachment `id` (`"namespace:name"`) with its
    /// default value constructor.
    ///
    /// Any valid lowercase `namespace:name` not already registered is accepted when submitted to
    /// [`Registrar::register_attachment_type`]; the registering package is recorded as the
    /// attachment's owner.
    pub const fn new(id: &'static str, default: fn() -> T) -> Self {
        Self {
            id,
            default,
            codec: None,
            copy_on_death: false,
            sync_codec: None,
        }
    }

    /// Persist this attachment to disk with `codec`.
    pub fn serialize(mut self, codec: AttachmentCodec<T>) -> Self {
        self.codec = Some(codec);
        self
    }

    /// Synchronize this attachment to clients using `codec`, independently of disk persistence.
    pub fn sync(mut self, codec: AttachmentCodec<T>) -> Self {
        self.sync_codec = Some(codec);
        self
    }

    /// Copy this attachment when a player respawns from death. Requires [`serialize`](Self::serialize).
    pub const fn copy_on_death(mut self) -> Self {
        self.copy_on_death = true;
        self
    }

    /// The full namespaced attachment ID (`"namespace:name"`).
    pub const fn id(&self) -> &'static str {
        self.id
    }

    /// Whether this descriptor has a persistence codec.
    pub const fn has_codec(&self) -> bool {
        self.codec.is_some()
    }

    /// Whether this descriptor synchronizes to clients.
    pub const fn is_synced(&self) -> bool {
        self.sync_codec.is_some()
    }

    /// Whether this descriptor copies on player death.
    pub const fn copy_on_death_enabled(&self) -> bool {
        self.copy_on_death
    }
}

impl<T: 'static> fmt::Debug for AttachmentDescriptor<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AttachmentDescriptor")
            .field("id", &self.id)
            .field("type", &type_name::<T>())
            .field("has_codec", &self.has_codec())
            .field("is_synced", &self.is_synced())
            .field("copy_on_death", &self.copy_on_death)
            .finish()
    }
}

type BuildAttachmentRecord = Box<
    dyn FnOnce(
            &'static str,
            &'static str,
            &'static str,
        ) -> Result<Arc<dyn Any + Send + Sync>, AttachmentError>
        + Send,
>;

type ProjectAttachmentSync = fn(
    &(dyn Any + Send + Sync),
    AttachmentSyncInput<'_>,
) -> Result<Option<Compound>, AttachmentError>;

/// Type-erased attachment descriptor accepted by [`AttachmentTypesRegistrar::submit_attachment`].
///
/// Built from a typed [`AttachmentDescriptor<T>`] via [`From`] / [`Into`].
// no wire: host-local registration descriptor carrying erased typed callbacks; never crosses the wire.
pub struct ErasedAttachmentDescriptor {
    id: &'static str,
    has_codec: bool,
    policy: AttachmentPolicy,
    build: BuildAttachmentRecord,
    project_sync: ProjectAttachmentSync,
}

impl<T: Send + Sync + 'static> From<AttachmentDescriptor<T>> for ErasedAttachmentDescriptor {
    fn from(descriptor: AttachmentDescriptor<T>) -> Self {
        fn project_sync<T: Send + Sync + 'static>(
            record: &(dyn Any + Send + Sync),
            input: AttachmentSyncInput<'_>,
        ) -> Result<Option<Compound>, AttachmentError> {
            let registration = record
                .downcast_ref::<AttachmentRegistration<T>>()
                .ok_or(AttachmentError::TypeMismatch)?;
            registration.checked.as_type().project_sync(input)
        }

        let id = descriptor.id;
        let has_codec = descriptor.codec.is_some();
        let policy = AttachmentPolicy {
            copy_on_death: descriptor.copy_on_death,
            sync: descriptor.sync_codec.is_some(),
        };
        let build: BuildAttachmentRecord = Box::new(move |namespace, name, owner| {
            let checked = CheckedAttachmentType::new(AttachmentType {
                namespace,
                name,
                default: descriptor.default,
                codec: descriptor.codec,
                copy_on_death: descriptor.copy_on_death,
                sync_codec: descriptor.sync_codec,
            })?;
            Ok(Arc::new(AttachmentRegistration { id, owner, checked })
                as Arc<dyn Any + Send + Sync>)
        });
        Self {
            id,
            has_codec,
            policy,
            build,
            project_sync: project_sync::<T>,
        }
    }
}

impl ErasedAttachmentDescriptor {
    /// The full namespaced attachment ID (`"namespace:name"`).
    pub fn id(&self) -> &'static str {
        self.id
    }

    /// Whether this descriptor has a persistence codec.
    pub fn has_codec(&self) -> bool {
        self.has_codec
    }

    /// Whether this descriptor synchronizes to clients.
    pub fn is_synced(&self) -> bool {
        self.policy.sync
    }

    /// Whether this descriptor copies on player death.
    pub fn copy_on_death_enabled(&self) -> bool {
        self.policy.copy_on_death
    }
}

impl fmt::Debug for ErasedAttachmentDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ErasedAttachmentDescriptor")
            .field("id", &self.id)
            .field("has_codec", &self.has_codec)
            .field("policy", &self.policy)
            .finish()
    }
}

pub(crate) struct AttachmentRegistration<T: 'static> {
    pub(crate) id: &'static str,
    pub(crate) owner: &'static str,
    pub(crate) checked: CheckedAttachmentType<T>,
}

/// Process-local identity of a single attachment registration, compared by pointer equality.
#[derive(Clone)]
pub(crate) struct AttachmentIdentity(Arc<dyn Any + Send + Sync>);

impl PartialEq for AttachmentIdentity {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::addr_eq(Arc::as_ptr(&self.0), Arc::as_ptr(&other.0))
    }
}

impl Eq for AttachmentIdentity {}

impl fmt::Debug for AttachmentIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AttachmentIdentity")
            .field(&Arc::as_ptr(&self.0))
            .finish()
    }
}

/// An opaque handle to an attachment registered during startup.
// no wire: host-local registration record holding an erased typed descriptor; never crosses the wire.
#[derive(Clone)]
pub struct RegisteredAttachment {
    id: &'static str,
    namespace: &'static str,
    name: &'static str,
    owner: &'static str,
    has_codec: bool,
    policy: AttachmentPolicy,
    record: Arc<dyn Any + Send + Sync>,
    project_sync: ProjectAttachmentSync,
}

impl RegisteredAttachment {
    /// The full namespaced attachment ID (`"namespace:name"`).
    pub fn id(&self) -> &'static str {
        self.id
    }

    /// The attachment ID's namespace part.
    pub fn namespace(&self) -> &'static str {
        self.namespace
    }

    /// The attachment ID's path/name part.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The package ID that registered this attachment.
    pub fn owner(&self) -> &'static str {
        self.owner
    }

    /// Whether this attachment has a persistence codec.
    pub fn has_codec(&self) -> bool {
        self.has_codec
    }

    /// Whether this attachment synchronizes to clients.
    pub fn is_synced(&self) -> bool {
        self.policy.sync
    }

    /// The attachment's persistence and sync policy.
    pub fn policy(&self) -> AttachmentPolicy {
        self.policy
    }

    pub(crate) fn matches_identity(&self, identity: &AttachmentIdentity) -> bool {
        std::ptr::addr_eq(Arc::as_ptr(&self.record), Arc::as_ptr(&identity.0))
    }

    pub(crate) fn downcast_key<T: Send + Sync + 'static>(
        &self,
    ) -> Result<AttachmentKey<T>, AttachmentError> {
        Arc::clone(&self.record)
            .downcast::<AttachmentRegistration<T>>()
            .map(AttachmentKey)
            .map_err(|_| AttachmentError::TypeMismatch)
    }

    pub(crate) fn project_sync(
        &self,
        input: AttachmentSyncInput<'_>,
    ) -> Result<Option<Compound>, AttachmentError> {
        (self.project_sync)(self.record.as_ref(), input)
    }
}

impl PartialEq for RegisteredAttachment {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::addr_eq(Arc::as_ptr(&self.record), Arc::as_ptr(&other.record))
    }
}

impl Eq for RegisteredAttachment {}

impl fmt::Debug for RegisteredAttachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegisteredAttachment")
            .field("id", &self.id)
            .field("owner", &self.owner)
            .field("has_codec", &self.has_codec)
            .field("policy", &self.policy)
            .finish()
    }
}

/// Validate an erased attachment descriptor and build its [`RegisteredAttachment`] record.
// hidden: shared between vocabulary fixtures and `sunrise-mod-api`'s registrar; not an authoring method.
#[doc(hidden)]
pub fn check_attachment_descriptor(
    descriptor: ErasedAttachmentDescriptor,
    owner: &'static str,
) -> Result<RegisteredAttachment, String> {
    if Identifier::parse_namespaced(descriptor.id).is_none() {
        return Err("explicit lowercase namespaced id required".to_owned());
    }
    let (namespace, name) = descriptor
        .id
        .split_once(':')
        .expect("parsed namespaced id contains ':'");
    let record = (descriptor.build)(namespace, name, owner).map_err(|err| match err {
        AttachmentError::InvalidCopyPolicy => "copy_on_death requires a serializer".to_owned(),
        other => format!("{other:?}"),
    })?;
    Ok(RegisteredAttachment {
        id: descriptor.id,
        namespace,
        name,
        owner,
        has_codec: descriptor.has_codec,
        policy: descriptor.policy,
        record,
        project_sync: descriptor.project_sync,
    })
}

/// Typed handle to an attachment registered during [`Hooks::register`](crate::Hooks::register).
///
/// Cloning an `AttachmentKey` is cheap (it shares the underlying registration) and preserves its
/// identity. A key obtained from a previous installation is refused with
/// [`AttachmentError::StaleKey`] even when a later installation registers the same ID and Rust
/// type.
// no wire: host-local typed handle with callback pointers and registration identity; never crosses the wire.
pub struct AttachmentKey<T: 'static>(Arc<AttachmentRegistration<T>>);

impl<T: 'static> Clone for AttachmentKey<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T: 'static> PartialEq for AttachmentKey<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<T: 'static> Eq for AttachmentKey<T> {}

impl<T: 'static> fmt::Debug for AttachmentKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AttachmentKey")
            .field("id", &self.0.id)
            .field("owner", &self.0.owner)
            .field("type", &type_name::<T>())
            .finish()
    }
}

impl<T: 'static> AttachmentKey<T> {
    /// The full namespaced attachment ID (`"namespace:name"`).
    pub fn id(&self) -> &'static str {
        self.0.id
    }

    /// The attachment ID's namespace part.
    pub fn namespace(&self) -> &'static str {
        self.0.checked.namespace()
    }

    /// The attachment ID's path/name part.
    pub fn name(&self) -> &'static str {
        self.0.checked.name()
    }

    /// The package ID that registered this attachment.
    pub fn owner(&self) -> &'static str {
        self.0.owner
    }

    /// Whether this attachment has a persistence codec.
    pub fn has_codec(&self) -> bool {
        self.0.checked.has_codec()
    }

    /// Whether this attachment synchronizes to clients.
    pub fn is_synced(&self) -> bool {
        self.0.checked.is_synced()
    }

    /// The attachment's persistence and sync policy.
    pub fn policy(&self) -> AttachmentPolicy {
        self.0.checked.policy()
    }

    /// Whether this key belongs to the currently installed registration epoch.
    pub fn is_live(&self) -> bool {
        self.live().is_ok()
    }

    pub(crate) fn live(&self) -> Result<&CheckedAttachmentType<T>, AttachmentError> {
        let current = installed()
            .and_then(|parts| parts.attachment_types.get(self.namespace()))
            .and_then(|types| types.get(self.name()))
            .ok_or(AttachmentError::StaleKey)?;
        if !std::ptr::addr_eq(Arc::as_ptr(&current.record), Arc::as_ptr(&self.0)) {
            return Err(AttachmentError::StaleKey);
        }
        Ok(&self.0.checked)
    }

    pub(crate) fn parts(&self) -> (&'static str, &'static str) {
        (self.namespace(), self.name())
    }

    pub(crate) fn descriptor(&self) -> &CheckedAttachmentType<T> {
        &self.0.checked
    }
}

impl<T: Send + Sync + 'static> AttachmentKey<T> {
    pub(crate) fn identity(&self) -> AttachmentIdentity {
        AttachmentIdentity(Arc::clone(&self.0) as Arc<dyn Any + Send + Sync>)
    }
}

impl<T: Clone + Send + Sync + 'static> AttachmentKey<T> {
    /// Read a value from `holder`, inserting the owner's default if absent (except on an
    /// original-player snapshot).
    pub fn get(&self, host: &mut dyn Host, holder: Holder) -> Result<T, AttachmentError> {
        self.live()?.get(host.attachments(), holder)
    }

    /// Whether `holder` currently carries a value; never inserts the default.
    pub fn has(&self, host: &mut dyn Host, holder: Holder) -> Result<bool, AttachmentError> {
        self.live()?.has(host.attachments(), holder)
    }

    /// Read a value from `holder` only if present; a saved or synced value decodes once.
    pub fn get_existing(
        &self,
        host: &mut dyn Host,
        holder: Holder,
    ) -> Result<Option<T>, AttachmentError> {
        self.live()?.get_existing(host.attachments(), holder)
    }

    /// Replace a value on `holder`.
    pub fn set(
        &self,
        host: &mut dyn Host,
        holder: Holder,
        value: T,
    ) -> Result<(), AttachmentError> {
        self.live()?.set(host.attachments(), holder, value)
    }

    /// Remove a value from `holder`, returning the previous value if one was present.
    pub fn remove(
        &self,
        host: &mut dyn Host,
        holder: Holder,
    ) -> Result<Option<T>, AttachmentError> {
        self.live()?.remove(host.attachments(), holder)
    }

    /// Queue the current value on `holder` for synchronization to clients that see it.
    pub fn sync_data(&self, host: &mut dyn Host, holder: Holder) -> Result<(), AttachmentError> {
        self.live()?.sync_data(host.attachments(), holder)
    }

    /// Read a value from `holder` through a [`LevelRandomHost`], inserting the default if absent.
    pub fn get_in(
        &self,
        level: &mut dyn LevelRandomHost,
        holder: Holder,
    ) -> Result<T, AttachmentError> {
        self.live()?.get(level.attachments(), holder)
    }

    /// Whether `holder` currently carries a value in `level`; never inserts the default.
    pub fn has_in(
        &self,
        level: &mut dyn LevelRandomHost,
        holder: Holder,
    ) -> Result<bool, AttachmentError> {
        self.live()?.has(level.attachments(), holder)
    }

    /// Read a value from `holder` in `level` only if present.
    pub fn get_existing_in(
        &self,
        level: &mut dyn LevelRandomHost,
        holder: Holder,
    ) -> Result<Option<T>, AttachmentError> {
        self.live()?.get_existing(level.attachments(), holder)
    }

    /// Replace a value on `holder` in `level`.
    pub fn set_in(
        &self,
        level: &mut dyn LevelRandomHost,
        holder: Holder,
        value: T,
    ) -> Result<(), AttachmentError> {
        self.live()?.set(level.attachments(), holder, value)
    }

    /// Remove a value from `holder` in `level`.
    pub fn remove_in(
        &self,
        level: &mut dyn LevelRandomHost,
        holder: Holder,
    ) -> Result<Option<T>, AttachmentError> {
        self.live()?.remove(level.attachments(), holder)
    }

    /// Queue the current value on `holder` in `level` for synchronization to clients.
    pub fn sync_data_in(
        &self,
        level: &mut dyn LevelRandomHost,
        holder: Holder,
    ) -> Result<(), AttachmentError> {
        self.live()?.sync_data(level.attachments(), holder)
    }
}

/// Object-safe attachment registration and lookup operations on [`Registrar`].
///
/// Prefer the typed inherent methods [`Registrar::register_attachment_type`] and
/// [`Registrar::attachment_key`] when the attachment value type `T` is known.
pub trait AttachmentTypesRegistrar {
    /// Register an erased attachment descriptor owned by the current package.
    ///
    /// Any valid lowercase `namespace:name` not already registered is accepted, regardless of
    /// whether `namespace` matches the registering package's ID. A refusal fails installation
    /// even when the caller ignores this result.
    fn submit_attachment(
        &mut self,
        descriptor: ErasedAttachmentDescriptor,
    ) -> Result<RegisteredAttachment, String>;

    /// Look up an attachment already registered earlier in this registration pass by full
    /// `"namespace:name"` ID.
    fn lookup_attachment(&self, id: &str) -> Result<RegisteredAttachment, AttachmentError>;
}

impl dyn Registrar + '_ {
    /// Register a typed attachment descriptor and return its issued [`AttachmentKey<T>`].
    ///
    /// Mirrors NeoForge's `RegisterEvent.register` for `NeoForgeRegistries.Keys.ATTACHMENT_TYPES`:
    /// any valid lowercase `namespace:name` not already registered is accepted, regardless of
    /// whether `namespace` matches this package's ID, and this package is recorded as the
    /// attachment's owner. Keep the returned key on the mod or Part; keys from prior
    /// installations are rejected with [`AttachmentError::StaleKey`]. A refusal fails
    /// installation even when ignored.
    pub fn register_attachment_type<T: Send + Sync + 'static>(
        &mut self,
        descriptor: AttachmentDescriptor<T>,
    ) -> Result<AttachmentKey<T>, String> {
        let record = self.submit_attachment(descriptor.into())?;
        Ok(record
            .downcast_key::<T>()
            .expect("submitted descriptor carries type T"))
    }

    /// Look up a typed [`AttachmentKey<T>`] registered earlier in this pass (for example by a
    /// dependency).
    ///
    /// Returns [`AttachmentError::Undeclared`] if `id` has not been registered yet and
    /// [`AttachmentError::TypeMismatch`] if `id` was registered with a different Rust type `T`.
    pub fn attachment_key<T: Send + Sync + 'static>(
        &self,
        id: &str,
    ) -> Result<AttachmentKey<T>, AttachmentError> {
        self.lookup_attachment(id)?.downcast_key::<T>()
    }
}

/// Project one attachment entry for client sync through the active registered descriptor.
///
/// Returns `None` when no active descriptor is registered for `namespace:name`, when the
/// active descriptor is not configured to sync, or when projection fails.
// hidden: engine-owned attachment sync projection; not an authoring method.
#[doc(hidden)]
pub fn project_sync_attachment(
    namespace: &str,
    name: &str,
    input: AttachmentSyncInput<'_>,
) -> Option<Compound> {
    let record = installed()?.attachment_types.get(namespace)?.get(name)?;
    record.project_sync(input).ok().flatten()
}

/// Validate use against the installed owner's declaration.
// hidden: shared engine and host adapter validation, not an authoring method.
#[doc(hidden)]
pub fn validate_attachment_use(
    namespace: &str,
    name: &str,
    sync: bool,
) -> Result<(), AttachmentError> {
    // Engine-owned storage work has no package identity. Every mod dispatch
    // enters its owning package scope before reaching this boundary.
    let Some(package) = crate::content::crash::current_native_mod() else {
        return Ok(());
    };
    validate_declared_attachment(
        installed().map(|parts| &parts.attachment_types),
        package,
        namespace,
        name,
        sync,
    )
}

// Owner policy: the owner's declaration suffices for all callers.
/// Check one id against collected declarations, before or after installation.
// hidden: engine-owned load validation; not an authoring operation.
#[doc(hidden)]
pub fn validate_declared_attachment(
    declarations: Option<&HashMap<&'static str, HashMap<&'static str, RegisteredAttachment>>>,
    caller: &str,
    namespace: &str,
    name: &str,
    sync: bool,
) -> Result<(), AttachmentError> {
    let Some(record) = declarations
        .and_then(|types| types.get(namespace))
        .and_then(|types| types.get(name))
    else {
        return Err(AttachmentError::Undeclared {
            package_id: caller.into(),
            attachment_id: format!("{namespace}:{name}"),
        });
    };
    if sync && !record.is_synced() {
        return Err(AttachmentError::InvalidSyncPolicy);
    }
    Ok(())
}

/// Build a `(RegisteredAttachment, AttachmentKey<T>)` pair for unit tests without a full
/// registration pass.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn registered_attachment_fixture<T: Send + Sync + 'static>(
    owner: &'static str,
    descriptor: AttachmentDescriptor<T>,
) -> (RegisteredAttachment, AttachmentKey<T>) {
    let record = check_attachment_descriptor(descriptor.into(), owner)
        .expect("fixture attachment descriptor must be valid");
    let key = record
        .downcast_key::<T>()
        .expect("fixture descriptor carries type T");
    (record, key)
}

/// Install a single attachment descriptor in the process-wide parts table for unit tests and
/// return its live [`AttachmentKey<T>`].
#[cfg(any(test, feature = "testing"))]
pub(crate) fn install_attachment_fixture<T: Send + Sync + 'static>(
    owner: &'static str,
    descriptor: AttachmentDescriptor<T>,
) -> AttachmentKey<T> {
    let (record, key) = registered_attachment_fixture(owner, descriptor);
    let namespace = record.namespace();
    let name = record.name();
    crate::parts::install_input(crate::parts::installation::InstalledInput {
        attachment_types: HashMap::from([(namespace, HashMap::from([(name, record)]))]),
        ..Default::default()
    });
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AttachmentStore, Attachments, Compound, LevelHost, Tag};

    fn i32_codec() -> AttachmentCodec<i32> {
        AttachmentCodec {
            encode: |n| Ok([("v".into(), Tag::Int(*n))].into()),
            decode: |tag| match tag.get(&"v".into()) {
                Some(Tag::Int(n)) => Ok(*n),
                _ => Err(AttachmentError::Decode),
            },
        }
    }

    struct FakeLevel(Attachments);
    impl LevelHost for FakeLevel {
        fn block_entity_data(&self, _: crate::BlockEntityRef) -> Option<Compound> {
            None
        }
    }
    impl LevelRandomHost for FakeLevel {
        fn attachments(&mut self) -> &mut dyn AttachmentStore {
            &mut self.0
        }
    }

    #[test]
    fn owner_declaration_allows_b_to_use_a_mana_and_names_missing_use() {
        let (record, _) =
            registered_attachment_fixture("a", AttachmentDescriptor::new("a:mana", || 0u32));
        let declarations = HashMap::from([("a", HashMap::from([("mana", record)]))]);
        assert_eq!(
            validate_declared_attachment(Some(&declarations), "b", "a", "mana", false),
            Ok(())
        );
        assert_eq!(
            validate_declared_attachment(Some(&declarations), "b", "a", "mana", true),
            Err(AttachmentError::InvalidSyncPolicy)
        );
        assert_eq!(
            validate_declared_attachment(Some(&declarations), "b", "a", "missing", false),
            Err(AttachmentError::Undeclared {
                package_id: "b".into(),
                attachment_id: "a:missing".into(),
            })
        );
    }

    #[test]
    fn descriptor_accepts_sync_without_persistence_codec_and_rejects_copy_on_death_without_codec() {
        let sync_only = AttachmentDescriptor::new("shared:mana", || 5i32).sync(i32_codec());
        assert!(!sync_only.has_codec());
        assert!(sync_only.is_synced());
        assert!(!sync_only.copy_on_death_enabled());
        let record = check_attachment_descriptor(sync_only.into(), "owner_mod")
            .expect("sync without persistence codec must be valid");
        assert_eq!(record.id(), "shared:mana");
        assert_eq!(record.namespace(), "shared");
        assert_eq!(record.name(), "mana");
        assert_eq!(record.owner(), "owner_mod");
        assert!(!record.has_codec());
        assert!(record.is_synced());

        let bad_copy = AttachmentDescriptor::new("owner_mod:mana", || 0i32).copy_on_death();
        assert_eq!(
            check_attachment_descriptor(bad_copy.into(), "owner_mod").unwrap_err(),
            "copy_on_death requires a serializer"
        );
    }

    #[test]
    fn attachment_key_detects_stale_keys_across_installations_even_with_same_id_and_type() {
        let first =
            install_attachment_fixture("mod_a", AttachmentDescriptor::new("mod_a:count", || 1i32));
        assert!(first.is_live());

        let mut host = crate::testing::FakeHost::default();
        assert_eq!(first.get(&mut host, Holder::Level), Ok(1));
        first.set(&mut host, Holder::Level, 9).unwrap();
        assert_eq!(first.get_existing(&mut host, Holder::Level), Ok(Some(9)));

        let mut level = FakeLevel(Attachments::default());
        assert_eq!(first.get_in(&mut level, Holder::Level), Ok(1));
        first.set_in(&mut level, Holder::Level, 4).unwrap();
        assert_eq!(first.remove_in(&mut level, Holder::Level), Ok(Some(4)));
        assert_eq!(first.has_in(&mut level, Holder::Level), Ok(false));

        // Reinstall with the exact same ID, owner, and Rust type: the first key must be stale.
        let second =
            install_attachment_fixture("mod_a", AttachmentDescriptor::new("mod_a:count", || 1i32));
        assert!(!first.is_live());
        assert!(second.is_live());
        assert_ne!(first, second);
        assert_eq!(
            first.get(&mut host, Holder::Level),
            Err(AttachmentError::StaleKey)
        );
        assert_eq!(
            first.has(&mut host, Holder::Level),
            Err(AttachmentError::StaleKey)
        );
        assert_eq!(
            first.set(&mut host, Holder::Level, 2),
            Err(AttachmentError::StaleKey)
        );
        assert_eq!(
            first.remove(&mut host, Holder::Level),
            Err(AttachmentError::StaleKey)
        );
        assert_eq!(
            first.sync_data(&mut host, Holder::Level),
            Err(AttachmentError::StaleKey)
        );
        assert_eq!(
            first.get_in(&mut level, Holder::Level),
            Err(AttachmentError::StaleKey)
        );
        assert_eq!(second.get_existing(&mut host, Holder::Level), Ok(Some(9)));
    }

    #[test]
    fn active_descriptor_projects_unread_saved_dual_codec_for_tracking_and_resync() {
        let disk_codec = AttachmentCodec {
            encode: |n: &i32| Ok([("disk".into(), Tag::Int(*n))].into()),
            decode: |tag: &Compound| match tag.get(&"disk".into()) {
                Some(Tag::Int(n)) => Ok(*n),
                _ => Err(AttachmentError::Decode),
            },
        };
        let sync_v1 = AttachmentCodec {
            encode: |n: &i32| Ok([("net".into(), Tag::Int(*n + 100))].into()),
            decode: |tag: &Compound| match tag.get(&"net".into()) {
                Some(Tag::Int(n)) => Ok(*n - 100),
                _ => Err(AttachmentError::Decode),
            },
        };
        let sync_v2 = AttachmentCodec {
            encode: |n: &i32| Ok([("net_v2".into(), Tag::Int(*n * 10))].into()),
            decode: |_: &Compound| Err(AttachmentError::Decode),
        };

        let key_v1 = install_attachment_fixture(
            "mod_a",
            AttachmentDescriptor::new("mod_a:dual", || 0i32)
                .serialize(disk_codec)
                .sync(sync_v1),
        );
        let mut level = FakeLevel(Attachments::default());
        key_v1.set_in(&mut level, Holder::Level, 7).unwrap();

        let mut saved = level.0.to_nbt();
        let Some(Tag::Compound(names)) = saved.get(&"mod_a".into()) else {
            panic!("saved namespace missing");
        };
        let Some(Tag::Compound(envelope)) = names.get(&"dual".into()) else {
            panic!("saved entry missing");
        };
        assert!(!envelope.contains_key(&"sync".into()));
        assert!(!envelope.contains_key(&"sync_value".into()));

        // Preserve an unregistered entry alongside the active dual-codec binding.
        saved.insert(
            "foreign".into(),
            Tag::Compound(
                [(
                    "opaque".into(),
                    Tag::Compound([("value".into(), Tag::Compound(Compound::new()))].into()),
                )]
                .into(),
            ),
        );

        let mut reloaded = FakeLevel(Attachments::from_nbt(saved.clone()));
        let expected_v1 = vec![sunrise_mod_runtime::attachments::AttachmentSync {
            namespace: "mod_a".into(),
            name: "dual".into(),
            value: Some([("net".into(), Tag::Int(107))].into()),
        }];
        assert_eq!(reloaded.0.synced(project_sync_attachment), expected_v1);
        key_v1.sync_data_in(&mut reloaded, Holder::Level).unwrap();
        assert_eq!(
            reloaded.0.take_sync_changes(project_sync_attachment),
            expected_v1
        );

        // Migrating the active descriptor's sync codec re-projects the unread saved entry.
        let key_v2 = install_attachment_fixture(
            "mod_a",
            AttachmentDescriptor::new("mod_a:dual", || 0i32)
                .serialize(disk_codec)
                .sync(sync_v2),
        );
        let expected_v2 = vec![sunrise_mod_runtime::attachments::AttachmentSync {
            namespace: "mod_a".into(),
            name: "dual".into(),
            value: Some([("net_v2".into(), Tag::Int(70))].into()),
        }];
        assert_eq!(reloaded.0.synced(project_sync_attachment), expected_v2);
        key_v2.sync_data_in(&mut reloaded, Holder::Level).unwrap();
        assert_eq!(
            reloaded.0.take_sync_changes(project_sync_attachment),
            expected_v2
        );

        // Unsyncing the active descriptor stops sending while keeping saved NBT intact.
        let _unsynced = install_attachment_fixture(
            "mod_a",
            AttachmentDescriptor::new("mod_a:dual", || 0i32).serialize(disk_codec),
        );
        assert!(reloaded.0.synced(project_sync_attachment).is_empty());
        assert_eq!(reloaded.0.to_nbt(), saved);
    }
}
