//! Load-owned attachment identities and owner-supplied callbacks.
use super::*;
use crate::{AttachmentCodec, AttachmentError, Holder};
use sunrise_mod_runtime::attachments::{AttachmentType, CheckedAttachmentType};

// no wire: an authoring descriptor contains process-local callbacks.
/// An attachment's saved name, default and optional persistence policies.
pub struct AttachmentDescriptor<T> {
    id: &'static str,
    default: fn() -> T,
    codec: Option<AttachmentCodec<T>>,
    sync: bool,
    copy_on_death: bool,
}

impl<T> AttachmentDescriptor<T> {
    /// Start a transient attachment. `id` must explicitly name the owning package's namespace.
    pub const fn new(id: &'static str, default: fn() -> T) -> Self {
        Self {
            id,
            default,
            codec: None,
            sync: false,
            copy_on_death: false,
        }
    }

    /// Encode this attachment in saves, and decode it on demand after loading.
    pub fn serialize(mut self, codec: AttachmentCodec<T>) -> Self {
        self.codec = Some(codec);
        self
    }

    /// Send the saved form to clients that track the holder, at transaction commit.
    /// Registration refuses this policy without a codec.
    pub const fn sync(mut self) -> Self {
        self.sync = true;
        self
    }

    /// Keep the saved form on player death and respawn.
    /// Registration refuses this policy without a codec.
    pub const fn copy_on_death(mut self) -> Self {
        self.copy_on_death = true;
        self
    }
}

type CheckDescriptor = Box<
    dyn FnOnce(&'static str, &'static str) -> Result<RegisteredAttachment, AttachmentError>
        + Send
        + Sync,
>;

// no wire: the descriptor owns a process-local callback that creates the typed record.
/// A descriptor with its value type erased for [`AttachmentTypesRegistrar::submit_attachment`].
pub struct ErasedAttachmentDescriptor {
    id: &'static str,
    check: CheckDescriptor,
}

impl ErasedAttachmentDescriptor {
    /// The explicitly namespaced name supplied by the author, not yet checked.
    pub fn id(&self) -> &'static str {
        self.id
    }
}

impl<T: Send + Sync + 'static> From<AttachmentDescriptor<T>> for ErasedAttachmentDescriptor {
    fn from(descriptor: AttachmentDescriptor<T>) -> Self {
        let AttachmentDescriptor {
            id,
            default,
            codec,
            sync,
            copy_on_death,
        } = descriptor;
        Self {
            id,
            check: Box::new(move |namespace, name| {
                let checked = CheckedAttachmentType::new(AttachmentType {
                    namespace,
                    name,
                    default,
                    codec,
                    sync,
                    copy_on_death,
                })?;
                Ok(RegisteredAttachment {
                    sync,
                    record: Arc::new(AttachmentRegistration {
                        id,
                        namespace,
                        name,
                        checked,
                    }),
                })
            }),
        }
    }
}

/// Check the callback policy and create the identity after the registrar has checked the name and owner.
// hidden: engine-side half of submitting the descriptor.
#[doc(hidden)]
pub fn check_attachment_descriptor(
    descriptor: ErasedAttachmentDescriptor,
    namespace: &'static str,
    name: &'static str,
) -> Result<RegisteredAttachment, AttachmentError> {
    (descriptor.check)(namespace, name)
}

// no wire: registration identity and checked callbacks are process-local.
/// The opaque registration shared by the owning package and all typed lookups.
#[derive(Clone)]
pub struct RegisteredAttachment {
    sync: bool,
    record: Arc<dyn Any + Send + Sync>,
}

struct AttachmentRegistration<T: 'static> {
    id: &'static str,
    namespace: &'static str,
    name: &'static str,
    checked: CheckedAttachmentType<T>,
}

impl RegisteredAttachment {
    /// Whether this registration supplies attachment values to clients.
    pub fn is_synced(&self) -> bool {
        self.sync
    }

    fn typed<T: Send + Sync + 'static>(self) -> Result<AttachmentKey<T>, AttachmentError> {
        self.record
            .downcast::<AttachmentRegistration<T>>()
            .map(AttachmentKey)
            .map_err(|_| AttachmentError::TypeMismatch)
    }
}

// no wire: a key holds a process-local registration identity and its typed callbacks.
/// An owned handle to one registration, obtained from the registrar and kept on a mod or Part.
/// A key from a different installation cannot read or write an attachment of the same name.
pub struct AttachmentKey<T: 'static>(Arc<AttachmentRegistration<T>>);

impl<T> Clone for AttachmentKey<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> AttachmentKey<T> {
    /// The fully qualified name used in saves; this name alone does not make a key live.
    pub fn id(&self) -> &'static str {
        self.0.id
    }

    pub(crate) fn live(&self) -> Result<&CheckedAttachmentType<T>, AttachmentError> {
        let current = installed()
            .and_then(|parts| parts.attachment_types.get(self.0.namespace))
            .and_then(|types| types.get(self.0.name));
        // The key keeps its allocation alive, so no later record can reuse this address.
        if !current.is_some_and(|current| {
            std::ptr::addr_eq(Arc::as_ptr(&current.record), Arc::as_ptr(&self.0))
        }) {
            return Err(AttachmentError::StaleKey);
        }
        Ok(&self.0.checked)
    }
}

impl<T: Clone + Send + Sync + 'static> AttachmentKey<T> {
    /// Read the value, inserting the owner's default if absent (except on an original player).
    pub fn get(&self, host: &mut dyn Host, holder: Holder) -> Result<T, AttachmentError> {
        self.live()?.get(host.attachments(), holder)
    }

    /// Whether a saved or decoded value exists, without inserting a default.
    pub fn has(&self, host: &mut dyn Host, holder: Holder) -> Result<bool, AttachmentError> {
        self.live()?.has(host.attachments(), holder)
    }

    /// Read a value only if present, decoding its saved form on demand.
    pub fn get_existing(
        &self,
        host: &mut dyn Host,
        holder: Holder,
    ) -> Result<Option<T>, AttachmentError> {
        self.live()?.get_existing(host.attachments(), holder)
    }

    /// Replace the value on an existing holder, using the registered codec and policies.
    pub fn set(
        &self,
        host: &mut dyn Host,
        holder: Holder,
        value: T,
    ) -> Result<(), AttachmentError> {
        self.live()?.set(host.attachments(), holder, value)
    }

    /// Remove the value and return its previous content, if any.
    pub fn remove(
        &self,
        host: &mut dyn Host,
        holder: Holder,
    ) -> Result<Option<T>, AttachmentError> {
        self.live()?.remove(host.attachments(), holder)
    }

    /// Queue the current value for the clients tracking the holder, if the type syncs.
    pub fn sync_data(&self, host: &mut dyn Host, holder: Holder) -> Result<(), AttachmentError> {
        self.live()?.sync_data(host.attachments(), holder)
    }
}

/// Attachment registration and lookup during [`crate::Hooks::register`].
pub trait AttachmentTypesRegistrar {
    /// Register the descriptor in its owning package. Prefer `register_attachment_type` to get
    /// a typed key. A refusal fails installation even when the caller ignores this result.
    fn submit_attachment(
        &mut self,
        descriptor: ErasedAttachmentDescriptor,
    ) -> Result<RegisteredAttachment, String>;

    /// Look up a descriptor already submitted in this registration pass, including another
    /// package's. Prefer `attachment_key` for a typed lookup.
    fn lookup_attachment(&self, id: &str) -> Result<RegisteredAttachment, AttachmentError>;
}

impl dyn Registrar + '_ {
    /// Register the descriptor once and keep the key on the mod or Part that uses it.
    /// Sync and copy-on-death both require a codec. A `side = "server"` package may use, but
    /// cannot declare, a synced attachment.
    pub fn register_attachment_type<T: Send + Sync + 'static>(
        &mut self,
        descriptor: AttachmentDescriptor<T>,
    ) -> Result<AttachmentKey<T>, String> {
        let id = descriptor.id;
        self.submit_attachment(descriptor.into())?
            .typed()
            .map_err(|_| format!("attachment {id}: registrar returned a different value type"))
    }

    /// Obtain an owned key that shares the owner's default, codec and policies. Look it up after
    /// the owner registers it (declare a dependency to order the packages). An unknown id gives
    /// `Undeclared`; a different `T` gives `TypeMismatch`.
    pub fn attachment_key<T: Send + Sync + 'static>(
        &self,
        id: &str,
    ) -> Result<AttachmentKey<T>, AttachmentError> {
        self.lookup_attachment(id)?.typed()
    }
}

/// Read-only access to the load-owned attachment registry, including without a world host.
// hidden: attachment metadata lookup; authors use typed keys and snapshots.
#[doc(hidden)]
pub trait AttachmentDeclarations {
    fn validate_attachment_type(
        &self,
        namespace: &str,
        name: &str,
        sync: bool,
    ) -> Result<(), AttachmentError>;
}

/// Validate use by consumers still operating on a storage-relative attachment type.
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
    let Some(record) = installed()
        .and_then(|parts| parts.attachment_types.get(namespace))
        .and_then(|types| types.get(name))
    else {
        return Err(AttachmentError::Undeclared {
            package_id: package.into(),
            attachment_id: format!("{namespace}:{name}"),
        });
    };
    if sync && !record.is_synced() {
        return Err(AttachmentError::InvalidSyncPolicy);
    }
    Ok(())
}
