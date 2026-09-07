//! Shannon entropy (in bits per byte) computed over byte windows.

#[cfg(not(target_family = "wasm"))]
use rayon::prelude::*;

/// Shannon entropy of a byte slice, normalized to `[0.0, 8.0]` bits per byte.
pub fn block_entropy(data: &[u8]) -> f32 {
    if data.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for &b in data {
        counts[b as usize] += 1;
    }
    let len = data.len() as f32;
    // Mathematically:
    // H = - sum_i (c_i / N) * log2(c_i / N)
    //   = - sum_i (c_i / N) * (log2(c_i) - log2(N))
    //   = log2(N) - (1 / N) * sum_i (c_i * log2(c_i))
    // Factoring out log2(N) and 1/N avoids up to 256 divisions and repeated log2(N)
    // calculations per block.
    let mut sum_c_log_c = 0.0f32;
    for &c in &counts {
        if c > 0 {
            let cf = c as f32;
            sum_c_log_c += cf * cf.log2();
        }
    }
    (len.log2() - sum_c_log_c / len).max(0.0)
}

/// Entropy of every contiguous `window`-sized block of `data`. One value per
/// block; pixel entropy is then looked up (and optionally interpolated) per
/// byte from this cache.
///
/// The host path runs under rayon. Wasm has no data parallelism on the main
/// thread (rayon needs threads, which need cross-origin isolation), so the
/// browser build runs the same map serially — the per-block work is identical,
/// only the scheduling differs.
#[cfg(not(target_family = "wasm"))]
pub fn block_entropies(data: &[u8], window: usize) -> Vec<f32> {
    let w = window.max(1);
    let nblocks = data.len().div_ceil(w);
    (0..nblocks)
        .into_par_iter()
        .map(|b| {
            let start = b * w;
            let end = (start + w).min(data.len());
            block_entropy(&data[start..end])
        })
        .collect()
}

#[cfg(target_family = "wasm")]
pub fn block_entropies(data: &[u8], window: usize) -> Vec<f32> {
    let w = window.max(1);
    let nblocks = data.len().div_ceil(w);
    (0..nblocks)
        .map(|b| {
            let start = b * w;
            let end = (start + w).min(data.len());
            block_entropy(&data[start..end])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_is_zero() {
        assert_eq!(block_entropy(&[0x41; 256]), 0.0);
    }

    #[test]
    fn full_range_is_eight() {
        let data: Vec<u8> = (0..=255u8).cycle().take(256).collect();
        let h = block_entropy(&data);
        assert!((h - 8.0).abs() < 0.01, "h={h}");
    }

    #[test]
    fn half_range_is_one() {
        let data: Vec<u8> = (0..256).map(|i| (i % 2) as u8 * 0x41).collect();
        let h = block_entropy(&data);
        assert!((h - 1.0).abs() < 0.01, "h={h}");
    }

    #[test]
    fn blocks_cover_file() {
        let data: Vec<u8> = (0..1000u16).map(|i| (i % 256) as u8).collect();
        let h = block_entropies(&data, 256);
        assert_eq!(h.len(), 4); // 1000 bytes -> ceil(1000/256) = 4 blocks
        assert_eq!(block_entropy(&data[..256]), h[0]);
    }
}
