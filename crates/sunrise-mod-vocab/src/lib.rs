//! The API vocabulary re-exported by `sunrise-mod-api`.
#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(ambiguous_glob_reexports)]
extern crate self as mod_api;

use std::sync::{Arc, OnceLock};

use serde_json::{Map, Value};
use std::time::Duration;

// First, so its `__sunrise_handler_marks!` is in textual scope for every module below.
include!(concat!(env!("OUT_DIR"), "/wire_generated.rs"));

mod generated_host {
    include!(concat!(env!("OUT_DIR"), "/mod_api_generated_host.rs"));
}

// no wire: type alias inherits the Wire policy of its tuple or Result target.
/// Result of accessing or decoding an attachment.
pub type AttachmentResult<T> = std::result::Result<T, AttachmentError>;

/// Marks one export as used: a call, on wasm, of a no-op import named `$name`. The module's
/// import section then lists the export it names; the link keeps the
/// import exactly while the code that calls this is linked.
///
/// `$id` names the function that holds the import. The generator gives every mark its own, because
/// rustc names an extern item by its Rust path, not by its `link_name`: two marks declared
/// as `mark` in the same function would be one import, the first one's.
///
/// The `direct` form declares the import in place, with no function of its own: a handler trait's
/// hidden marks method expands it (`__sunrise_handler_marks!`), so the import sits in the
/// method's body, which is instantiated only for an implementor the mod's own module holds. A
/// function of its own would be one the module exports, whether or not anything calls it.
///
/// This is the one place the vocabulary lets unsafe code through (the crate denies it): the
/// allowance covers the one generated function and nothing else.
#[doc(hidden)]
#[macro_export]
macro_rules! __sunrise_import {
    (direct $id:ident, $name:literal) => {{
        #[cfg(target_arch = "wasm32")]
        #[allow(unsafe_code)]
        {
            #[link(wasm_import_module = "sunrise")]
            unsafe extern "C" {
                #[link_name = $name]
                fn $id();
            }
            // SAFETY: a no-op import of the game's table, which takes and returns nothing: it
            // reads and writes no memory of the module, so calling it has no precondition, and
            // the loader refuses the module, naming it, when the game's table lacks the name.
            unsafe { $id() }
        }
    }};
    ($id:ident, $name:literal) => {{
        #[cfg(target_arch = "wasm32")]
        {
            #[allow(unsafe_code)]
            fn $id() {
                #[link(wasm_import_module = "sunrise")]
                unsafe extern "C" {
                    #[link_name = $name]
                    fn mark();
                }
                // SAFETY: a no-op import of the game's table, which takes and returns nothing: it
                // reads and writes no memory of the module, so calling it has no precondition, and
                // the loader refuses the module, naming it, when the game's table lacks the name.
                unsafe { mark() }
            }
            $id();
        }
    }};
}

mod generated {
    include!(concat!(env!("OUT_DIR"), "/mod_api_generated.rs"));
}

include!(concat!(env!("OUT_DIR"), "/mod_api_exports_root.rs"));
