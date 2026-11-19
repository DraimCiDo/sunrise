//! Java's `String.hashCode`: `s[0]*31^(n-1) + ... + s[n-1]` over the string's UTF-16 units, in
//! wrapping `int` arithmetic. Bit-exact on every input, including lone surrogates; nothing allocates.
//!
//! Long strings fold 32 units at a time into 32 lanes, each lane multiplied by 31^32 per block, then
//! weighted by its power of 31 at the end: the same sum in the same wrapping arithmetic, reordered,
//! which wrapping multiplication and addition (a ring modulo 2^32) leave exact. This module depends
//! on nothing else in the crate, so that `tools/string_hash_kit` can time byte-identical copies of it.

/// Strings at least this long take the lanes; shorter ones fold four units per step.
const LANES_FROM: usize = 32;
/// 31^0 to 31^32, wrapping.
const POWERS: [u32; 33] = {
    let mut p = [1u32; 33];
    let mut i = 1;
    while i < 33 {
        p[i] = p[i - 1].wrapping_mul(31);
        i += 1;
    }
    p
};

/// `String.hashCode` of a string's UTF-16 units.
#[inline]
pub fn java_string_hash(units: &[u16]) -> i32 {
    fold(0, units) as i32
}

/// `String.hashCode` of the Java string holding `text`'s UTF-16 units (`text.encode_utf16()`),
/// without collecting them.
#[inline]
pub fn java_string_hash_str(text: &str) -> i32 {
    let bytes = text.as_bytes();
    if bytes.is_ascii() {
        // Each byte is one UTF-16 unit.
        return fold(0, bytes) as i32;
    }
    hash_mixed(text) as i32
}

/// A string with non-ASCII characters: its UTF-16 units, decoded into a small buffer on the stack
/// and folded a buffer at a time.
#[inline(never)]
fn hash_mixed(text: &str) -> u32 {
    let mut buffer = [0u16; 2 * LANES_FROM];
    let mut filled = 0;
    let mut hash = 0;
    for c in text.chars() {
        filled += c.encode_utf16(&mut buffer[filled..]).len();
        if filled > buffer.len() - 2 {
            hash = fold(hash, &buffer[..filled]);
            filled = 0;
        }
    }
    fold(hash, &buffer[..filled])
}

/// `hash` continued over `units`: the hash of a string whose first part hashed to `hash`.
#[inline(always)]
fn fold<T: Copy + Into<u32>>(hash: u32, units: &[T]) -> u32 {
    if units.len() >= LANES_FROM {
        fold_long(hash, units)
    } else {
        fold_short(hash, units)
    }
}

#[inline(always)]
fn fold_short<T: Copy + Into<u32>>(mut hash: u32, units: &[T]) -> u32 {
    let mut quads = units.chunks_exact(4);
    for q in &mut quads {
        let sum = q[0]
            .into()
            .wrapping_mul(POWERS[3])
            .wrapping_add(q[1].into().wrapping_mul(POWERS[2]))
            .wrapping_add(q[2].into().wrapping_mul(POWERS[1]))
            .wrapping_add(q[3].into());
        hash = hash.wrapping_mul(POWERS[4]).wrapping_add(sum);
    }
    for &u in quads.remainder() {
        hash = hash.wrapping_mul(31).wrapping_add(u.into());
    }
    hash
}

#[inline(never)]
fn fold_long<T: Copy + Into<u32>>(hash: u32, units: &[T]) -> u32 {
    fold_lanes(hash, units)
}

/// Lane `j` holds the units at `j` of every 32-unit block, each block multiplying the lanes by
/// 31^32; the lane then weighs 31^(31-j). `hash` starts in the last lane, whose weight is 1.
#[inline(always)]
fn fold_lanes<T: Copy + Into<u32>>(hash: u32, units: &[T]) -> u32 {
    let mut lanes = [0u32; LANES_FROM];
    lanes[LANES_FROM - 1] = hash;
    let mut blocks = units.chunks_exact(LANES_FROM);
    for block in &mut blocks {
        for (lane, &u) in lanes.iter_mut().zip(block) {
            *lane = lane.wrapping_mul(POWERS[LANES_FROM]).wrapping_add(u.into());
        }
    }
    let mut hash = 0u32;
    for (j, lane) in lanes.iter().enumerate() {
        hash = hash.wrapping_add(lane.wrapping_mul(POWERS[LANES_FROM - 1 - j]));
    }
    fold_short(hash, blocks.remainder())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values from `String.hashCode` on JDK 25 (`tools/test_string_hash_parity.py` checks every
    /// one- and two-unit string and billions of random ones).
    #[test]
    fn known_java_hashes() {
        assert_eq!(java_string_hash_str(""), 0);
        assert_eq!(java_string_hash_str("minecraft"), 695_073_197);
        assert_eq!(java_string_hash_str("\u{1F600}"), 1_772_899);
        assert_eq!(
            java_string_hash_str("the quick brown fox jumps over the lazy dog, twice!"),
            1_262_420_192
        );
        let mixed = "caf\u{e9} \u{4e16}\u{754c} \u{1F600} abcdefghijklmnopqrstuvwxyz0123456789 \u{ff}\u{100}";
        assert_eq!(java_string_hash_str(mixed), 1_354_549_821);
        let units: Vec<u16> = mixed.encode_utf16().collect();
        assert_eq!(java_string_hash(&units), 1_354_549_821);
        assert_eq!(java_string_hash(&[0xD800]), 55_296);
    }
}
