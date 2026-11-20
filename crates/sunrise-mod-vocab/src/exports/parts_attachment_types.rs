//! Typed attachments and engine attachment declaration validation.
pub use crate::parts::{
    AttachmentDescriptor, AttachmentKey, ErasedAttachmentDescriptor, RegisteredAttachment,
};
// hidden: the engine's registry lookup; authors bind typed keys and snapshots.
#[doc(hidden)]
pub use crate::parts::AttachmentDeclarations;
mod __private {
    pub use crate::parts::{check_attachment_descriptor, validate_attachment_use};
}
