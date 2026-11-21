//! Typed attachment descriptors, registration records, issued keys, and declaration validation.
pub use crate::parts::{
    AttachmentDescriptor, AttachmentKey, ErasedAttachmentDescriptor, RegisteredAttachment,
};
mod __private {
    pub use crate::parts::{
        check_attachment_descriptor, project_sync_attachment, registered_copy_on_death,
        validate_attachment_use, validate_attachment_write, validate_declared_attachment,
    };
}
