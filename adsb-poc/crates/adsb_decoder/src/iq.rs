//! SC16 IQ ingest (interleaved little-endian I/Q).

use std::path::Path;

use memmap2::Mmap;

pub const SAMPLE_RATE: f64 = 2_400_000.0;

/// Load interleaved little-endian SC16 samples (I,Q,I,Q,...).
#[allow(dead_code)] // unit tests / alternate ingest; hot path is `load_sc16_magnitude`
pub fn load_sc16(path: &Path) -> std::io::Result<Vec<i16>> {
    let file = std::fs::File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    let mut out = Vec::with_capacity(mmap.len() / 2);
    for chunk in mmap.chunks_exact(2) {
        out.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }
    Ok(out)
}

/// Mmap SC16 and convert straight to magnitude (no intermediate `Vec<i16>`).
#[allow(dead_code)] // unit tests / oneshot helper; CLI uses chunked pipeline
pub fn load_sc16_magnitude(path: &Path) -> std::io::Result<Vec<u16>> {
    let file = std::fs::File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    Ok(magnitude_from_le_bytes(&mmap))
}

/// Convert IQ to magnitude via hardware `f32` sqrt (dump1090/readsb-style).
///
/// Integer `u32::isqrt` is far slower on large captures; `f32` sqrt matches
/// demod thresholds closely enough that DF17/18 yield stays at parity.
#[allow(dead_code)] // unit tests; hot path is `load_sc16_magnitude`
pub fn magnitude(iq: &[i16]) -> Vec<u16> {
    let mut mag = Vec::with_capacity(iq.len() / 2);
    for c in iq.chunks_exact(2) {
        mag.push(mag_sample(c[0] as f32, c[1] as f32));
    }
    mag
}

/// Convert interleaved little-endian SC16 bytes to magnitude samples.
pub fn magnitude_from_le_bytes(bytes: &[u8]) -> Vec<u16> {
    let mut mag = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let i = i16::from_le_bytes([chunk[0], chunk[1]]) as f32;
        let q = i16::from_le_bytes([chunk[2], chunk[3]]) as f32;
        mag.push(mag_sample(i, q));
    }
    mag
}

#[inline]
fn mag_sample(i: f32, q: f32) -> u16 {
    // Same clamp as the old isqrt path; max |SC16| mag is ~46341 < u16::MAX.
    (i * i + q * q).sqrt().min(u16::MAX as f32) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnitude_matches_integer_isqrt_on_typical_samples() {
        let iq = [
            0i16, 0, 3, 4, -3, 4, 1000, -1000, 32767, 0, 0, -32768, 20000, 20000,
        ];
        let got = magnitude(&iq);
        let expect: Vec<u16> = iq
            .chunks_exact(2)
            .map(|c| {
                let i = c[0] as i32;
                let q = c[1] as i32;
                ((i * i + q * q) as u32).isqrt().min(u16::MAX as u32) as u16
            })
            .collect();
        // f32 may differ by ±1 on large sums; allow that, require exact on small.
        assert_eq!(&got[..3], &expect[..3]);
        for (g, e) in got.iter().zip(expect.iter()) {
            assert!((*g as i32 - *e as i32).abs() <= 1, "{g} vs {e}");
        }
    }

    #[test]
    fn magnitude_from_bytes_matches_iq_slice() {
        let iq = [10i16, -20, 300, 400];
        let mut bytes = Vec::new();
        for v in iq {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(magnitude_from_le_bytes(&bytes), magnitude(&iq));
    }

    #[test]
    fn load_sc16_and_magnitude_roundtrip_via_tempfile() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("adsb_iq_test_{}.sc16", std::process::id()));
        let iq = [1i16, 2, -3, 4];
        let mut bytes = Vec::new();
        for v in iq {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(&path, &bytes).unwrap();
        let loaded = load_sc16(&path).unwrap();
        assert_eq!(loaded, iq);
        assert_eq!(load_sc16_magnitude(&path).unwrap(), magnitude(&iq));
        let _ = std::fs::remove_file(&path);
    }
}
