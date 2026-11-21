//! Typed attachment descriptors, registration records, issued keys, and declaration validation.
pub use crate::parts::{
    AttachmentDescriptor, AttachmentKey, ErasedAttachmentDescriptor, RegisteredAttachment,
};
mod __private {
    pub use crate::parts::{
        check_attachment_descriptor, validate_attachment_use, validate_declared_attachment,
    };
}
