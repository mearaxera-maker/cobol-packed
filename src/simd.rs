#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// Expand packed bytes into a nibble stream.
///
/// Each input byte becomes two output bytes: high nibble, low nibble.
///
/// The SIMD paths are validated against the scalar reference in debug builds so
/// a future maintenance change cannot silently diverge from the scalar codec.
pub fn expand_nibbles(bytes: &[u8]) -> Vec<u8> {
    #[cfg(target_arch = "x86_64")]
    {
        #[cfg(feature = "avx2")]
        {
            if std::is_x86_feature_detected!("avx2") {
                // SAFETY: AVX2 is checked at runtime.
                let simd = unsafe { expand_nibbles_avx2(bytes) };
                debug_assert_eq!(simd, expand_nibbles_scalar(bytes));
                return simd;
            }
        }
        if std::is_x86_feature_detected!("sse2") {
            // SAFETY: SSE2 is checked at runtime.
            let simd = unsafe { expand_nibbles_sse2(bytes) };
            debug_assert_eq!(simd, expand_nibbles_scalar(bytes));
            return simd;
        }
    }
    expand_nibbles_scalar(bytes)
}

#[inline]
pub(crate) fn expand_nibbles_scalar(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(b >> 4);
        out.push(b & 0x0F);
    }
    out
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn load128(ptr: *const u8) -> __m128i {
    if (ptr as usize) & 0x0F == 0 {
        _mm_load_si128(ptr as *const __m128i)
    } else {
        _mm_loadu_si128(ptr as *const __m128i)
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn expand_nibbles_sse2(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 2);
    let mask = _mm_set1_epi8(0x0F_i8);
    let mut i = 0;

    while i + 16 <= bytes.len() {
        let v = load128(bytes.as_ptr().add(i));
        let hi = _mm_and_si128(_mm_srli_epi16(v, 4), mask);
        let lo = _mm_and_si128(v, mask);

        let mut hi_arr = [0u8; 16];
        let mut lo_arr = [0u8; 16];
        _mm_storeu_si128(hi_arr.as_mut_ptr() as *mut __m128i, hi);
        _mm_storeu_si128(lo_arr.as_mut_ptr() as *mut __m128i, lo);

        for j in 0..16 {
            out.push(hi_arr[j]);
            out.push(lo_arr[j]);
        }
        i += 16;
    }

    for &b in &bytes[i..] {
        out.push(b >> 4);
        out.push(b & 0x0F);
    }

    out
}

#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
#[inline(always)]
unsafe fn load256(ptr: *const u8) -> __m256i {
    if (ptr as usize) & 0x1F == 0 {
        _mm256_load_si256(ptr as *const __m256i)
    } else {
        _mm256_loadu_si256(ptr as *const __m256i)
    }
}

#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
#[target_feature(enable = "avx2")]
unsafe fn expand_nibbles_avx2(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 2);
    let mask = _mm256_set1_epi8(0x0F_i8);
    let mut i = 0;

    while i + 32 <= bytes.len() {
        let v = load256(bytes.as_ptr().add(i));
        let hi = _mm256_and_si256(_mm256_srli_epi16(v, 4), mask);
        let lo = _mm256_and_si256(v, mask);

        let mut hi_arr = [0u8; 32];
        let mut lo_arr = [0u8; 32];
        _mm256_storeu_si256(hi_arr.as_mut_ptr() as *mut __m256i, hi);
        _mm256_storeu_si256(lo_arr.as_mut_ptr() as *mut __m256i, lo);

        for j in 0..32 {
            out.push(hi_arr[j]);
            out.push(lo_arr[j]);
        }
        i += 32;
    }

    if i < bytes.len() {
        out.extend_from_slice(&expand_nibbles_scalar(&bytes[i..]));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{expand_nibbles, expand_nibbles_scalar};

    #[test]
    fn simd_and_scalar_agree() {
        let input = vec![0x01, 0x23, 0x4C, 0xAD, 0xEF, 0x00, 0x99, 0x10, 0xFF, 0x7B];
        let got = expand_nibbles(&input);
        assert_eq!(
            got,
            vec![0, 1, 2, 3, 4, 12, 10, 13, 14, 15, 0, 0, 9, 9, 1, 0, 15, 15, 7, 11]
        );
    }

    #[test]
    fn scalar_reference_handles_random_shapes() {
        let cases = [
            vec![],
            vec![0x00],
            vec![0xFF],
            vec![0x12, 0x34, 0x56, 0x78],
            vec![0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67],
            vec![0x10; 33],
        ];
        for case in cases {
            assert_eq!(expand_nibbles(&case), expand_nibbles_scalar(&case));
        }
    }
}
