use super::case::{ascii_swap_case, eq_lowered_case};
use smallvec::SmallVec;

// Byte frequency table stolen from memchr
const BYTE_FREQUENCIES: [u8; 256] = [
    55, 52, 51, 50, 49, 48, 47, 46, 45, 103, 242, 66, 67, 229, 44, 43, // 0x00
    42, 41, 40, 39, 38, 37, 36, 35, 34, 33, 56, 32, 31, 30, 29, 28, // 0x10
    255, 148, 164, 149, 136, 160, 155, 173, 221, 222, 134, 122, 232, 202, 215, 224, // 0x20
    208, 220, 204, 187, 183, 179, 177, 168, 178, 200, 226, 195, 154, 184, 174, 126, // 0x30
    120, 191, 157, 194, 170, 189, 162, 161, 150, 193, 142, 137, 171, 176, 185,
    167, // 0x40 A-O
    186, 112, 175, 192, 188, 156, 140, 143, 123, 133, 128, 147, 138, 146, 114,
    223, // 0x50 P-_
    151, 249, 216, 238, 236, 253, 227, 218, 230, 247, 135, 180, 241, 233, 246,
    244, // 0x60 a-o
    231, 139, 245, 243, 251, 235, 201, 196, 240, 214, 152, 182, 205, 181, 127,
    27, // 0x70 p-DEL
    212, 211, 210, 213, 228, 197, 169, 159, 131, 172, 105, 80, 98, 96, 97, 81, // 0x80
    207, 145, 116, 115, 144, 130, 153, 121, 107, 132, 109, 110, 124, 111, 82, 108, // 0x90
    118, 141, 113, 129, 119, 125, 165, 117, 92, 106, 83, 72, 99, 93, 65, 79, // 0xa0
    166, 237, 163, 199, 190, 225, 209, 203, 198, 217, 219, 206, 234, 248, 158, 239, // 0xb0
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // 0xc0
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // 0xd0
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // 0xe0
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, // 0xf0
];

#[inline]
fn rank(lower: u8) -> u8 {
    if lower.is_ascii_lowercase() {
        BYTE_FREQUENCIES[lower as usize].max(BYTE_FREQUENCIES[ascii_swap_case(lower) as usize])
    } else {
        BYTE_FREQUENCIES[lower as usize]
    }
}

/// Pick two needle positions with the rarest bytes (case-insensitive)
fn select_rare_pair(needle_lower: &[u8]) -> (usize, usize) {
    debug_assert!(needle_lower.len() >= 2);

    let mut best1 = (u8::MAX, 0usize); // (rank, position)
    let mut best2 = (u8::MAX, 1usize);

    for (i, &b) in needle_lower.iter().enumerate() {
        let r = rank(b);
        if r < best1.0 {
            best2 = best1;
            best1 = (r, i);
        } else if r < best2.0 && i != best1.1 {
            best2 = (r, i);
        }
    }

    let i1 = best1.1.min(best2.1);
    let i2 = best1.1.max(best2.1);
    (i1, i2)
}

/// Extract a 16-bit bitmask from a NEON comparison result (each byte 0x00 or 0xFF)
/// Bit *i* of the result corresponds to byte *i* of the input vector
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
#[inline]
unsafe fn neon_movemask(v: core::arch::aarch64::uint8x16_t) -> u16 {
    use core::arch::aarch64::*;

    // AND each byte with its bit-position mask, then horizontally sum each half.
    static BITS: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];
    let bit_mask = unsafe { vld1q_u8(BITS.as_ptr()) };
    let masked = vandq_u8(v, bit_mask);
    let lo = vaddv_u8(vget_low_u8(masked));
    let hi = vaddv_u8(vget_high_u8(masked));
    (lo as u16) | ((hi as u16) << 8)
}

/// AVX2 packed-pair kernel: scan 32 haystack positions per iteration,
/// checking two rare bytes (case-insensitive) simultaneously
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn find_packed_pair_avx2(
    haystack: &[u8],
    needle_lower: &[u8],
    i1: usize,
    i2: usize,
) -> Option<usize> {
    use core::arch::x86_64::*;

    let n = needle_lower.len();
    let hlen = haystack.len();
    let ptr = haystack.as_ptr();
    let last_start = hlen - n; // last valid match-start position

    let b1 = needle_lower[i1];
    let b1_alt = if b1.is_ascii_lowercase() {
        ascii_swap_case(b1)
    } else {
        b1
    };
    let b2 = needle_lower[i2];
    let b2_alt = if b2.is_ascii_lowercase() {
        ascii_swap_case(b2)
    } else {
        b2
    };

    let v1_lo = _mm256_set1_epi8(b1 as i8);
    let v1_hi = _mm256_set1_epi8(b1_alt as i8);
    let v2_lo = _mm256_set1_epi8(b2 as i8);
    let v2_hi = _mm256_set1_epi8(b2_alt as i8);

    // Loads come from ptr+offset+i1 and ptr+offset+i2, so we need offset + max(i1,i2) + 31 < hlen.
    let max_idx = i1.max(i2);
    let max_offset = hlen.saturating_sub(max_idx + 32);
    let mut offset = 0usize;

    while offset <= max_offset {
        let chunk1 = unsafe { _mm256_loadu_si256(ptr.add(offset + i1) as *const __m256i) };
        let chunk2 = unsafe { _mm256_loadu_si256(ptr.add(offset + i2) as *const __m256i) };

        // Case-insensitive match: OR both case variants, then AND the two positions.
        let eq1 = _mm256_or_si256(
            _mm256_cmpeq_epi8(chunk1, v1_lo),
            _mm256_cmpeq_epi8(chunk1, v1_hi),
        );
        let eq2 = _mm256_or_si256(
            _mm256_cmpeq_epi8(chunk2, v2_lo),
            _mm256_cmpeq_epi8(chunk2, v2_hi),
        );

        let mut mask = _mm256_movemask_epi8(_mm256_and_si256(eq1, eq2)) as u32;

        // Candidates are visited in increasing position order, so the first
        // verified candidate is the leftmost match
        while mask != 0 {
            let bit = mask.trailing_zeros() as usize;
            let candidate = offset + bit;
            if candidate > last_start {
                return None;
            }
            if unsafe { eq_lowered_case(ptr.add(candidate), needle_lower) } {
                return Some(candidate);
            }
            mask &= mask - 1;
        }

        offset += 32;
    }

    // handle remaining characters
    if offset <= last_start {
        let rare_pos = if rank(needle_lower[i1]) <= rank(needle_lower[i2]) {
            i1
        } else {
            i2
        };
        let rare_byte = needle_lower[rare_pos];
        let tail_start = offset + rare_pos;
        let tail_end = last_start + rare_pos + 1;
        if tail_start < tail_end {
            let tail_space = &haystack[tail_start..tail_end];
            if rare_byte.is_ascii_lowercase() {
                for pos in memchr::memchr2_iter(rare_byte, ascii_swap_case(rare_byte), tail_space) {
                    let candidate = offset + pos;
                    if unsafe { eq_lowered_case(ptr.add(candidate), needle_lower) } {
                        return Some(candidate);
                    }
                }
            } else {
                for pos in memchr::memchr_iter(rare_byte, tail_space) {
                    let candidate = offset + pos;
                    if unsafe { eq_lowered_case(ptr.add(candidate), needle_lower) } {
                        return Some(candidate);
                    }
                }
            }
        }
    }

    None
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn find_packed_pair_neon(
    haystack: &[u8],
    needle_lower: &[u8],
    i1: usize,
    i2: usize,
) -> Option<usize> {
    use core::arch::aarch64::*;

    let n = needle_lower.len();
    let hlen = haystack.len();
    let ptr = haystack.as_ptr();
    let last_start = hlen - n;

    let b1 = needle_lower[i1];
    let b1_alt = if b1.is_ascii_lowercase() {
        ascii_swap_case(b1)
    } else {
        b1
    };
    let b2 = needle_lower[i2];
    let b2_alt = if b2.is_ascii_lowercase() {
        ascii_swap_case(b2)
    } else {
        b2
    };

    let v1_lo = vdupq_n_u8(b1);
    let v1_hi = vdupq_n_u8(b1_alt);
    let v2_lo = vdupq_n_u8(b2);
    let v2_hi = vdupq_n_u8(b2_alt);

    let max_idx = i1.max(i2);
    let max_offset = hlen.saturating_sub(max_idx + 16);
    let mut offset = 0usize;

    while offset <= max_offset {
        let chunk1 = unsafe { vld1q_u8(ptr.add(offset + i1)) };
        let chunk2 = unsafe { vld1q_u8(ptr.add(offset + i2)) };

        // Case-insensitive match: OR both case variants, then AND the two positions.
        let eq1 = vorrq_u8(vceqq_u8(chunk1, v1_lo), vceqq_u8(chunk1, v1_hi));
        let eq2 = vorrq_u8(vceqq_u8(chunk2, v2_lo), vceqq_u8(chunk2, v2_hi));

        let mut mask = unsafe { neon_movemask(vandq_u8(eq1, eq2)) };

        while mask != 0 {
            let bit = mask.trailing_zeros() as usize;
            let candidate = offset + bit;
            if candidate > last_start {
                return None;
            }
            if unsafe { eq_lowered_case(ptr.add(candidate), needle_lower) } {
                return Some(candidate);
            }
            mask &= mask - 1;
        }

        offset += 16;
    }

    // Tail: remaining positions that couldn't fill a full vector.
    if offset <= last_start {
        let rare_pos = if rank(needle_lower[i1]) <= rank(needle_lower[i2]) {
            i1
        } else {
            i2
        };
        let rare_byte = needle_lower[rare_pos];
        let tail_start = offset + rare_pos;
        let tail_end = last_start + rare_pos + 1;
        if tail_start < tail_end {
            let tail_space = &haystack[tail_start..tail_end];
            if rare_byte.is_ascii_lowercase() {
                for pos in memchr::memchr2_iter(rare_byte, ascii_swap_case(rare_byte), tail_space) {
                    let candidate = offset + pos;
                    if unsafe { eq_lowered_case(ptr.add(candidate), needle_lower) } {
                        return Some(candidate);
                    }
                }
            } else {
                for pos in memchr::memchr_iter(rare_byte, tail_space) {
                    let candidate = offset + pos;
                    if unsafe { eq_lowered_case(ptr.add(candidate), needle_lower) } {
                        return Some(candidate);
                    }
                }
            }
        }
    }

    None
}

fn find_first_byte_with_memchr(haystack: &[u8], needle_lower: &[u8]) -> Option<usize> {
    let n = needle_lower.len();
    debug_assert!(n >= 1 && n <= haystack.len());

    let search_space = &haystack[..=haystack.len() - n];
    let first = needle_lower[0];

    if first.is_ascii_lowercase() {
        let alt = ascii_swap_case(first);
        for pos in memchr::memchr2_iter(first, alt, search_space) {
            if unsafe { eq_lowered_case(haystack.as_ptr().add(pos), needle_lower) } {
                return Some(pos);
            }
        }
    } else {
        for pos in memchr::memchr_iter(first, search_space) {
            if unsafe { eq_lowered_case(haystack.as_ptr().add(pos), needle_lower) } {
                return Some(pos);
            }
        }
    }
    None
}

/// ASCII case-insensitive substring search returning the leftmost match
/// position. `needle_lower` must be pre-lowercased (ASCII).
// pub because it is used in out of the crate benchmarks
#[doc(hidden)] // it's pub only for benches
pub fn find(haystack: &[u8], needle_lower: &[u8]) -> Option<usize> {
    let n = needle_lower.len();
    if n == 0 {
        return Some(0);
    }
    if n > haystack.len() {
        return None;
    }

    if n == 1 {
        let first = needle_lower[0];
        return if first.is_ascii_lowercase() {
            memchr::memchr2(first, ascii_swap_case(first), haystack)
        } else {
            memchr::memchr(first, haystack)
        };
    }

    #[cfg_attr(
        not(any(target_arch = "x86_64", target_arch = "aarch64")),
        allow(unused_variables)
    )]
    let (i1, i2) = select_rare_pair(needle_lower);

    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            // Need enough haystack for at least one vector load.
            let max_idx = i1.max(i2);
            if haystack.len() >= max_idx + 32 {
                return unsafe { find_packed_pair_avx2(haystack, needle_lower, i1, i2) };
            }
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // Packed-pair wins when the first byte is common (memchr2 drowns in
        // false positives), but a rare first byte (z, q, x, ...) makes
        // memchr2's raw throughput dominate. Threshold 200 on the frequency
        // table splits common letters (s=243, e=253) from rare ones (z=152).
        let first_byte_rank = rank(needle_lower[0]);
        let max_idx = i1.max(i2);
        if first_byte_rank >= 200 && haystack.len() >= max_idx + 16 {
            return unsafe { find_packed_pair_neon(haystack, needle_lower, i1, i2) };
        }
    }

    // fallbacks to memchr based implementation cause we still have it and it supports more SIMD backends
    // TODO convert all the supported backend by memchr and get rid of the fallback
    find_first_byte_with_memchr(haystack, needle_lower)
}

/// A case insensitive find that works better with smaller strings, doesn't unwrap a complicated
/// AVX backend we use for grep because only cpu flags check takes usually more time than find itself
pub fn find_case_insensitive_short(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    debug_assert!(haystack.len() < 1024);
    let mut needle_lower: SmallVec<[u8; 64]> = SmallVec::from_slice(needle);
    needle_lower.make_ascii_lowercase();

    find(haystack, &needle_lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_find(haystack: &[u8], needle_lower: &[u8]) -> Option<usize> {
        if needle_lower.is_empty() {
            return Some(0);
        }
        if needle_lower.len() > haystack.len() {
            return None;
        }
        haystack
            .windows(needle_lower.len())
            .position(|w| w.eq_ignore_ascii_case(needle_lower))
    }

    #[test]
    fn basic_case_insensitive() {
        assert_eq!(find(b"Hello World", b"hello"), Some(0));
        assert_eq!(find(b"Hello World", b"world"), Some(6));
        assert_eq!(find(b"NOMORE bugs", b"nomore"), Some(0));
        assert_eq!(find(b"Hello World", b"xyz"), None);
        assert!(find(b"Hello World", b"o w").is_some());
    }

    #[test]
    fn edge_cases() {
        assert_eq!(find(b"ab", b"ab"), Some(0));
        assert_eq!(find(b"AB", b"ab"), Some(0));
        assert_eq!(find(b"a", b"ab"), None);
        assert_eq!(find(b"anything", b""), Some(0));
        assert_eq!(find(b"", b"x"), None);
        assert_eq!(find(b"xxA", b"a"), Some(2));
        assert_eq!(find(b"xx:", b":"), Some(2));
    }

    #[test]
    fn returns_leftmost_match() {
        assert_eq!(find(b"foo FOO foo", b"foo"), Some(0));
        let mut big = vec![b'.'; 300];
        big[100..103].copy_from_slice(b"FoO");
        big[200..203].copy_from_slice(b"foo");
        assert_eq!(find(&big, b"foo"), Some(100));
    }

    #[test]
    fn non_letter_bytes_do_not_case_fold() {
        // '[' (0x5B) and '{' (0x7B) differ only in bit 5 but are not letters.
        // A fold implemented as a bare `| 0x20` would falsely match these.
        assert_eq!(find(b"A[", b"a{"), None);
        assert_eq!(find(b"x@y", b"x`y"), None);
        assert_eq!(find(b"a]b", b"a}b"), None);
        assert_eq!(find(b"A{", b"a{"), Some(0));
    }

    #[test]
    fn matches_reference_on_random_inputs() {
        // Deterministic xorshift PRNG — no external deps.
        let mut state = 0x9E3779B97F4A7C15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };

        // Alphabet with letters, both-case pairs, and 0x20-differing symbols.
        let alphabet = b"aAbBzZ [{@`]}^~_0.\n";
        for _ in 0..2000 {
            let hlen = (next() % 200) as usize;
            let nlen = (next() % 8) as usize;
            let haystack: Vec<u8> = (0..hlen)
                .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
                .collect();
            let needle: Vec<u8> = (0..nlen)
                .map(|_| alphabet[(next() % alphabet.len() as u64) as usize].to_ascii_lowercase())
                .collect();

            assert_eq!(
                find(&haystack, &needle),
                reference_find(&haystack, &needle),
                "mismatch for haystack={:?} needle={:?}",
                haystack,
                needle,
            );
        }
    }

    #[test]
    fn long_haystack_simd_paths() {
        let haystack =
            b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaTHIS_IS_A_LONG_NEEDLE_TESTbbbbbbbbbbbbbbbbbb";
        assert_eq!(find(haystack, b"this_is_a_long_needle_test"), Some(32));
        assert_eq!(find(haystack, b"this_is_a_long_needle_testz"), None);

        // Needle >= 16 bytes exercises SIMD verify.
        let haystack2 = b"int STRUCT MUTEX *LOCK(struct mutex *lock) { return 0; }";
        assert_eq!(find(haystack2, b"struct mutex *lock"), Some(4));

        let upper_hay = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        assert_eq!(find(upper_hay, b"qrstuvwxyz0123456789a"), Some(16));
        assert_eq!(find(upper_hay, b"qrstuvwxyz01234567899"), None);

        // Needle at very end / very start.
        let end_hay = b"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxfind_me";
        assert_eq!(find(end_hay, b"find_me"), Some(end_hay.len() - 7));
        assert_eq!(find(end_hay, b"xx"), Some(0));

        // 1KB haystack with needle near the end.
        let mut big = vec![b'z'; 1024];
        big[1000..1010].copy_from_slice(b"hElLo_WoRl");
        assert_eq!(find(&big, b"hello_wo"), Some(1000));
        assert_eq!(find(&big, b"hello_world"), None);
    }

    #[test]
    fn rare_pair_selection() {
        let (i1, i2) = select_rare_pair(b"nomore");
        let ranks: Vec<u8> = b"nomore".iter().map(|&b| rank(b)).collect();
        let (r1, r2) = (ranks[i1], ranks[i2]);
        for (i, &r) in ranks.iter().enumerate() {
            if i != i1 && i != i2 {
                assert!(r1 <= r || r2 <= r, "pair ({i1},{i2}) not optimal");
            }
        }
    }
}
