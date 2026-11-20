//! Typed attachments and engine attachment declaration validation.
pub use crate::parts::{
    AttachmentDescriptor, AttachmentKey, ErasedAttachmentDescriptor, RegisteredAttachment,
};
mod __private {
    pub use crate::parts::{check_attachment_descriptor, validate_attachment_use};
}
