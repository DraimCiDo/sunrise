//! Compatibility for native packages whose `uses.json` predates the schema 3 hash recipe.
//! The former method/function hashes included a WebAssembly eligibility bit that did not
//! affect native callers. Accept either old value, but only for a legacy table. Delete this
//! module and its callers once no published registry package carries a schema 2 or older list.

use crate::{FunctionContract, MethodContract, Surface, contract_hash};
use serde::Serialize;
use std::collections::BTreeMap;

/// The surface schema that contains hashes from the old recipe.
pub(crate) const LEGACY_SURFACE_SCHEMA: u32 = 2;
pub(crate) const LEGACY_USES_SCHEMA: u32 = 2;

pub(crate) struct LegacyHashes<'a>(BTreeMap<&'a str, Contract<'a>>);

enum Contract<'a> {
    Method(&'a MethodContract),
    Function(&'a FunctionContract),
}

impl<'a> LegacyHashes<'a> {
    pub(crate) fn new(target: &'a Surface) -> Self {
        let methods = target.traits.iter().flat_map(|trait_| &trait_.methods);
        let functions = target.functions.iter().chain(&target.handles);
        Self(
            methods
                .map(|m| (m.id.as_str(), Contract::Method(&m.contract)))
                .chain(functions.map(|f| (f.id.as_str(), Contract::Function(&f.contract))))
                .collect(),
        )
    }

    /// `None` means this record's hash recipe never changed.
    pub(crate) fn matches(&self, id: &str, old_hash: &str) -> Option<bool> {
        let contract = self.0.get(id)?;
        // Field order and omission rules are part of the compact JSON being hashed.
        #[derive(Serialize)]
        struct Method<'a> {
            signature: &'a str,
            required: bool,
            wasm: bool,
            #[serde(skip_serializing_if = "<[String]>::is_empty")]
            refs: &'a [String],
        }
        #[derive(Serialize)]
        struct Function<'a> {
            signature: &'a str,
            #[serde(skip_serializing_if = "str::is_empty")]
            impl_generics: &'a str,
            wasm: bool,
            #[serde(skip_serializing_if = "<[String]>::is_empty")]
            refs: &'a [String],
        }
        Some([true, false].into_iter().any(|wasm| {
            let hash = match contract {
                Contract::Method(m) => contract_hash(&Method {
                    signature: &m.signature,
                    required: m.required,
                    wasm,
                    refs: &m.refs,
                }),
                Contract::Function(f) => contract_hash(&Function {
                    signature: &f.signature,
                    impl_generics: &f.impl_generics,
                    wasm,
                    refs: &f.refs,
                }),
            };
            hash == old_hash
        }))
    }
}
