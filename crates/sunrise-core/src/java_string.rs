//! Java's `String.hashCode`: `s[0]*31^(n-1) + ... + s[n-1]` over the string's UTF-16 units, in
//! wrapping `int` arithmetic. Bit-exact on every input, including lone surrogates; nothing allocates.
//!
//! On x86-64, strings of sixteen units or more are folded eight units at a time with the SSE2
//! operations every x86-64 CPU has, through `safe_arch`'s safe wrappers, so the crate stays free of
//! `unsafe`. Each unit is weighted by its power of 31 straight from a table (a power of 31 modulo
//! 2^32, since wrapping multiplication and addition are a ring), so the units of a block are
//! independent sums and no multiplication waits for another: the same value as `h = 31 * h + c`,
//! summed in another order. Elsewhere it is the plain loop, and below sixteen units four units a step. This module
//! depends on nothing else in the crate, so that `tools/string_hash_kit` can time byte-identical
//! copies of it.

use simd::{WIDE_FROM, fold, fold_short};

/// `String.hashCode` of a string's UTF-16 units.
#[inline]
pub fn java_string_hash(units: &[u16]) -> i32 {
    fold(0, units) as i32
}

/// `String.hashCode` of the Java string holding `text`'s UTF-16 units (`text.encode_utf16()`),
/// without collecting them.
#[inline]
pub fn java_string_hash_str(text: &str) -> i32 {
    // For ASCII text each byte is one UTF-16 unit. Short text is hashed while looking for a byte
    // that is not ASCII; longer text is checked first, so that text that is not ASCII does not
    // pay for a fold it throws away.
    let bytes = text.as_bytes();
    if bytes.len() < WIDE_FROM {
        let (hash, non_ascii) = fold_short(0, bytes);
        if !non_ascii {
            return hash as i32;
        }
    } else if bytes.is_ascii() {
        return fold(0, bytes) as i32;
    }
    hash_mixed(text) as i32
}

/// A string with non-ASCII characters: the fold over the units its characters encode to.
#[inline(never)]
fn hash_mixed(text: &str) -> u32 {
    text.encode_utf16().fold(0u32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(u32::from(unit))
    })
}

#[cfg(target_arch = "x86_64")]
mod simd {
    use safe_arch::*;

    /// 31^0 to 31^BLOCK, wrapping.
    const POWERS: [u32; BLOCK + 1] = {
        let mut p = [1u32; BLOCK + 1];
        let mut i = 1;
        while i <= BLOCK {
            p[i] = p[i - 1].wrapping_mul(31);
            i += 1;
        }
        p
    };
    /// Units folded per block, each block as long a run of vector steps as the weight tables hold.
    const BLOCK: usize = 256;
    /// Units per vector step.
    const STEP: usize = 8;

    /// Units from here on take vector steps.
    pub(super) const WIDE_FROM: usize = 2 * STEP;

    /// `hash` continued over `units`: the hash of a string whose first part hashed to `hash`.
    #[inline(always)]
    pub(super) fn fold<T: Unit>(hash: u32, units: &[T]) -> u32 {
        if units.len() < WIDE_FROM {
            return fold_short(hash, units).0;
        }
        fold_wide(hash, units)
    }

    /// `fold` for under sixteen units, four a step, in few enough registers that an empty string
    /// costs next to nothing; also whether a unit is 0x80 or more (not ASCII, for bytes).
    #[inline(always)]
    pub(super) fn fold_short<T: Unit>(mut hash: u32, units: &[T]) -> (u32, bool) {
        let mut high = 0u32;
        if units.len() < 4 {
            for &u in units {
                high |= u.into();
                hash = hash.wrapping_mul(31).wrapping_add(u.into());
            }
            return (hash, high >= 0x80);
        }
        let (quads, rest) = units.as_chunks::<4>();
        for q in quads {
            let [a, b, c, d] = q.map(Into::into);
            high |= a | b | c | d;
            let sum = a
                .wrapping_mul(POWERS[3])
                .wrapping_add(b.wrapping_mul(POWERS[2]))
                .wrapping_add(c.wrapping_mul(31))
                .wrapping_add(d);
            hash = hash.wrapping_mul(POWERS[4]).wrapping_add(sum);
        }
        for &u in rest {
            high |= u.into();
            hash = hash.wrapping_mul(31).wrapping_add(u.into());
        }
        (hash, high >= 0x80)
    }

    /// Sixteen units or more: vector steps.
    #[inline(never)]
    fn fold_wide<T: Unit>(mut hash: u32, units: &[T]) -> u32 {
        let (blocks, rest) = units.as_chunks::<BLOCK>();
        for block in blocks {
            hash = hash
                .wrapping_mul(POWERS[BLOCK])
                .wrapping_add(weighted_sum(block));
        }
        let (head, body) = rest.split_at(rest.len() % STEP);
        for &u in head {
            hash = hash.wrapping_mul(31).wrapping_add(u.into());
        }
        if !body.is_empty() {
            hash = hash
                .wrapping_mul(POWERS[body.len()])
                .wrapping_add(weighted_sum(body));
        }
        hash
    }

    /// A UTF-16 unit, or an ASCII byte standing for one.
    pub(super) trait Unit: Copy + Into<u32> {
        const ZERO: Self;
        /// Two vector steps of units as 16-bit lanes.
        fn load(units: &[Self; 2 * STEP]) -> [m128i; 2];
    }
    impl Unit for u16 {
        const ZERO: u16 = 0;
        #[inline(always)]
        fn load(units: &[u16; 2 * STEP]) -> [m128i; 2] {
            let (a, b) = units.split_at(STEP);
            [
                m128i::from(<[u16; STEP]>::try_from(a).unwrap()),
                m128i::from(<[u16; STEP]>::try_from(b).unwrap()),
            ]
        }
    }
    impl Unit for u8 {
        const ZERO: u8 = 0;
        #[inline(always)]
        fn load(units: &[u8; 2 * STEP]) -> [m128i; 2] {
            let bytes = m128i::from(*units);
            let zero = zeroed_m128i();
            [
                unpack_low_i8_m128i(bytes, zero),
                unpack_high_i8_m128i(bytes, zero),
            ]
        }
    }

    /// Each unit's weight in a block, 31^(BLOCK-1-k), as the signed low half and the high half that
    /// makes up the difference: `weight == lo + (hi << 16)` modulo 2^32.
    const WEIGHT_LO: [[i16; STEP]; BLOCK / STEP] = {
        let mut w = [[0i16; STEP]; BLOCK / STEP];
        let mut k = 0;
        while k < BLOCK {
            w[k / STEP][k % STEP] = POWERS[BLOCK - 1 - k] as u16 as i16;
            k += 1;
        }
        w
    };
    const WEIGHT_HI: [[i16; STEP]; BLOCK / STEP] = {
        let mut w = [[0i16; STEP]; BLOCK / STEP];
        let mut k = 0;
        while k < BLOCK {
            let lo = WEIGHT_LO[k / STEP][k % STEP] as i32 as u32;
            w[k / STEP][k % STEP] = (POWERS[BLOCK - 1 - k].wrapping_sub(lo) >> 16) as u16 as i16;
            k += 1;
        }
        w
    };
    /// 0x8000 times the sum of the first `8 * n` low halves' weights, for each n: what biasing every
    /// unit by 0x8000 (below) takes off the low products.
    const BIAS: [u32; BLOCK / STEP + 1] = {
        let mut bias = [0u32; BLOCK / STEP + 1];
        let mut n = 1;
        while n <= BLOCK / STEP {
            let mut sum = 0u32;
            let mut k = 0;
            while k < STEP {
                sum = sum.wrapping_add(WEIGHT_LO[BLOCK / STEP - n][k] as i32 as u32);
                k += 1;
            }
            bias[n] = bias[n - 1].wrapping_add(sum.wrapping_mul(0x8000));
            n += 1;
        }
        bias
    };

    /// The units (a multiple of 8, at most a block) each weighted by 31 to the number of units after
    /// it: the weights are the last of a block's. Eight units over a multiple of 16 go in front of
    /// the rest as a step of 16 with eight zeros before them, which weigh nothing.
    ///
    /// A unit `c` times a weight is `c * lo + (c * hi << 16)` for the weight's halves. `lo` is taken as
    /// signed and `c` biased by 0x8000 (undone by `BIAS`), so that `lo` products are the signed 16-bit
    /// pairs a multiply-add (`pmaddwd`) sums; `hi` products only matter modulo 2^16, which a 16-bit
    /// multiply (`pmullw`) gives.
    #[inline(always)]
    fn weighted_sum<T: Unit>(units: &[T]) -> u32 {
        let (front, rest) = units.split_at(units.len() % (2 * STEP));
        let pairs = rest.as_chunks::<{ 2 * STEP }>().0;
        let rows = 2 * (pairs.len() + usize::from(!front.is_empty()));
        let first = BLOCK / STEP - rows;
        let bias = set_splat_i16_m128i(i16::MIN);
        let (mut low, mut high) = (zeroed_m128i(), zeroed_m128i());
        let mut row = first;
        let mut step = |chunk: &[T; 2 * STEP], row: usize| {
            for (k, v) in T::load(chunk).into_iter().enumerate() {
                let (lo, hi) = (
                    m128i::from(WEIGHT_LO[row + k]),
                    m128i::from(WEIGHT_HI[row + k]),
                );
                low = add_i32_m128i(low, mul_i16_horizontal_add_m128i(bitxor_m128i(v, bias), lo));
                high = add_i16_m128i(high, mul_i16_keep_low_m128i(v, hi));
            }
        };
        if !front.is_empty() {
            let mut padded = [T::ZERO; 2 * STEP];
            padded[STEP..].copy_from_slice(front);
            step(&padded, row);
            row += 2;
        }
        for pair in pairs {
            step(pair, row);
            row += 2;
        }
        // Sum the 16-bit lanes of `high` in pairs, as the top halves of 32-bit lanes.
        let high =
            shl_imm_u32_m128i::<16>(mul_i16_horizontal_add_m128i(high, set_splat_i16_m128i(1)));
        let [a, b, c, d]: [u32; 4] = add_i32_m128i(low, high).into();
        a.wrapping_add(b)
            .wrapping_add(c)
            .wrapping_add(d)
            .wrapping_add(BIAS[rows])
    }
}

/// The plain loop, where there are no vector steps to take.
#[cfg(not(target_arch = "x86_64"))]
mod simd {
    /// A UTF-16 unit, or an ASCII byte standing for one.
    pub(super) trait Unit: Copy + Into<u32> {}
    impl Unit for u8 {}
    impl Unit for u16 {}

    /// Units from here on would take vector steps.
    pub(super) const WIDE_FROM: usize = 16;

    /// `hash` continued over `units`: the hash of a string whose first part hashed to `hash`.
    pub(super) fn fold<T: Unit>(hash: u32, units: &[T]) -> u32 {
        fold_short(hash, units).0
    }

    /// `fold`, and whether a unit is 0x80 or more (not ASCII, for bytes).
    pub(super) fn fold_short<T: Unit>(hash: u32, units: &[T]) -> (u32, bool) {
        let hash = units
            .iter()
            .fold(hash, |h, &u| h.wrapping_mul(31).wrapping_add(u.into()));
        (hash, units.iter().any(|&u| u.into() >= 0x80))
    }
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

    /// Every length around the vector step and the block, on the extreme units (0x8000 and 0xFFFF
    /// are where the biasing matters), against the definition `h = 31 * h + c`.
    #[test]
    fn every_length_matches_the_definition() {
        let naive = |units: &[u16]| {
            units
                .iter()
                .fold(0i32, |h, &c| h.wrapping_mul(31).wrapping_add(i32::from(c)))
        };
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        for len in (0..=3 * 256 + 9).chain([1024, 1025, 4099]) {
            for fill in [0u16, 0x7F, 0x8000, 0xFFFF, 1] {
                let units = vec![fill; len];
                assert_eq!(java_string_hash(&units), naive(&units), "{len} x {fill:#x}");
            }
            let units: Vec<u16> = (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state >> 20) as u16
                })
                .collect();
            assert_eq!(java_string_hash(&units), naive(&units), "{len} random");
            let ascii: String = units
                .iter()
                .map(|&u| char::from(b' ' + (u % 95) as u8))
                .collect();
            let ascii_units: Vec<u16> = ascii.encode_utf16().collect();
            assert_eq!(
                java_string_hash_str(&ascii),
                naive(&ascii_units),
                "{len} ascii"
            );
            let text: String = ascii
                .chars()
                .enumerate()
                .map(|(i, c)| if i % 5 == 3 { '\u{e9}' } else { c })
                .collect();
            let text_units: Vec<u16> = text.encode_utf16().collect();
            assert_eq!(
                java_string_hash_str(&text),
                naive(&text_units),
                "{len} latin1 text"
            );
            let wide: String = ascii
                .chars()
                .enumerate()
                .map(|(i, c)| match i % 7 {
                    2 => '\u{4e16}',
                    5 => '\u{1F600}',
                    _ => c,
                })
                .collect();
            let wide_units: Vec<u16> = wide.encode_utf16().collect();
            assert_eq!(
                java_string_hash_str(&wide),
                naive(&wide_units),
                "{len} 3- and 4-byte text"
            );
        }
    }
}
