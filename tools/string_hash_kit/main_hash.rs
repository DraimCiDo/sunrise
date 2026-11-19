//! The hash main had before this change (sunrise-core `random::java_string_hash` at bbbf904a63, which took any
//! iterator of UTF-16 units), with the two ways its callers fed it: `&[u16]` through `iter().copied()` and `&str`
//! through `encode_utf16()`. tools/string_hash_kit/run.py times byte-identical copies of it beside java_string.rs.

pub fn java_string_hash_iter(name: impl IntoIterator<Item = u16>) -> i32 {
    name.into_iter().fold(0i32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(unit as i32)
    })
}

pub fn java_string_hash(units: &[u16]) -> i32 {
    java_string_hash_iter(units.iter().copied())
}

pub fn java_string_hash_str(text: &str) -> i32 {
    java_string_hash_iter(text.encode_utf16())
}
