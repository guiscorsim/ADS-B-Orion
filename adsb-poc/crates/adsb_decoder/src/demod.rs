//! Mode S / ADS-B demodulation at 2.4 Msps (hand-rolled).
//!
//! Aligned with readsb's 2.4 MHz demod (wiedehopf-tuned slice coefficients +
//! preamble threshold + ICAO filter scoring).

use std::collections::HashSet;

use crate::mode_s::{crc24, df11_for_each_1bit_repair, df11_syndrome_iid_ok, icao_from_aa};

/// readsb default `--preamble-threshold` (58).
const PREAMBLE_THRESHOLD: i32 = 58;

/// Preamble span before the first data bit (samples @ 2.4 Msps).
pub const PREAMBLE_SAMPLES: usize = 19;
/// Max samples needed after preamble start to finish a long Mode S frame.
pub const MESSAGE_SAMPLES: usize = 270;
/// Trailing samples retained across mag chunks so edge preambles still decode.
pub const DEMOD_OVERLAP: usize = PREAMBLE_SAMPLES + MESSAGE_SAMPLES;

/// `(global_sample_index, raw_bytes, nbits)`.
pub type RawFrame = (usize, [u8; 14], usize);

// readsb/wiedehopf correlation kernels (hand-tuned vs classic dump1090-fa).
#[inline]
fn slice_phase0(m: &[u16]) -> i32 {
    18 * m[0] as i32 - 15 * m[1] as i32 - 3 * m[2] as i32
}
#[inline]
fn slice_phase1(m: &[u16]) -> i32 {
    14 * m[0] as i32 - 5 * m[1] as i32 - 9 * m[2] as i32
}
#[inline]
fn slice_phase2(m: &[u16]) -> i32 {
    16 * m[0] as i32 + 5 * m[1] as i32 - 20 * m[2] as i32
}
#[inline]
fn slice_phase3(m: &[u16]) -> i32 {
    7 * m[0] as i32 + 11 * m[1] as i32 - 18 * m[2] as i32
}
#[inline]
fn slice_phase4(m: &[u16]) -> i32 {
    4 * m[0] as i32 + 15 * m[1] as i32 - 20 * m[2] as i32 + m[3] as i32
}

#[inline]
fn bit(v: i32) -> u8 {
    u8::from(v > 0)
}

fn decode_byte(m: &[u16], phase: &mut usize, ptr: &mut usize) -> Option<u8> {
    if *ptr + 20 >= m.len() {
        return None;
    }
    let p = *ptr;
    let (b, new_phase, advance) = match *phase {
        0 => {
            let b = (bit(slice_phase0(&m[p..])) << 7)
                | (bit(slice_phase2(&m[p + 2..])) << 6)
                | (bit(slice_phase4(&m[p + 4..])) << 5)
                | (bit(slice_phase1(&m[p + 7..])) << 4)
                | (bit(slice_phase3(&m[p + 9..])) << 3)
                | (bit(slice_phase0(&m[p + 12..])) << 2)
                | (bit(slice_phase2(&m[p + 14..])) << 1)
                | bit(slice_phase4(&m[p + 16..]));
            (b, 1usize, 19usize)
        }
        1 => {
            let b = (bit(slice_phase1(&m[p..])) << 7)
                | (bit(slice_phase3(&m[p + 2..])) << 6)
                | (bit(slice_phase0(&m[p + 5..])) << 5)
                | (bit(slice_phase2(&m[p + 7..])) << 4)
                | (bit(slice_phase4(&m[p + 9..])) << 3)
                | (bit(slice_phase1(&m[p + 12..])) << 2)
                | (bit(slice_phase3(&m[p + 14..])) << 1)
                | bit(slice_phase0(&m[p + 17..]));
            (b, 2, 19)
        }
        2 => {
            let b = (bit(slice_phase2(&m[p..])) << 7)
                | (bit(slice_phase4(&m[p + 2..])) << 6)
                | (bit(slice_phase1(&m[p + 5..])) << 5)
                | (bit(slice_phase3(&m[p + 7..])) << 4)
                | (bit(slice_phase0(&m[p + 10..])) << 3)
                | (bit(slice_phase2(&m[p + 12..])) << 2)
                | (bit(slice_phase4(&m[p + 14..])) << 1)
                | bit(slice_phase1(&m[p + 17..]));
            (b, 3, 19)
        }
        3 => {
            let b = (bit(slice_phase3(&m[p..])) << 7)
                | (bit(slice_phase0(&m[p + 3..])) << 6)
                | (bit(slice_phase2(&m[p + 5..])) << 5)
                | (bit(slice_phase4(&m[p + 7..])) << 4)
                | (bit(slice_phase1(&m[p + 10..])) << 3)
                | (bit(slice_phase3(&m[p + 12..])) << 2)
                | (bit(slice_phase0(&m[p + 15..])) << 1)
                | bit(slice_phase2(&m[p + 17..]));
            (b, 4, 19)
        }
        _ => {
            let b = (bit(slice_phase4(&m[p..])) << 7)
                | (bit(slice_phase1(&m[p + 3..])) << 6)
                | (bit(slice_phase3(&m[p + 5..])) << 5)
                | (bit(slice_phase0(&m[p + 8..])) << 4)
                | (bit(slice_phase2(&m[p + 10..])) << 3)
                | (bit(slice_phase4(&m[p + 12..])) << 2)
                | (bit(slice_phase1(&m[p + 15..])) << 1)
                | bit(slice_phase3(&m[p + 17..]));
            (b, 0, 20)
        }
    };
    *phase = new_phase;
    *ptr += advance;
    Some(b)
}

fn slice_message(m: &[u16], start: usize, try_phase: usize) -> Option<([u8; 14], usize)> {
    let mut ptr = start + 19 + try_phase / 5;
    let mut phase = try_phase % 5;
    let mut msg = [0u8; 14];
    msg[0] = decode_byte(m, &mut phase, &mut ptr)?;
    let df = msg[0] >> 3;
    let nbytes = match df {
        0 | 4 | 5 | 11 => 7,
        16 | 17 | 18 | 20 | 21 => 14,
        _ => return None,
    };
    for slot in msg.iter_mut().take(nbytes).skip(1) {
        *slot = decode_byte(m, &mut phase, &mut ptr)?;
    }
    Some((msg, nbytes * 8))
}

fn score(msg: &[u8], nbits: usize, known: &HashSet<u32>) -> i32 {
    let nbytes = nbits / 8;
    if nbytes >= 7 && msg[..7].iter().all(|&b| b == 0) {
        return -2;
    }
    let df = msg[0] >> 3;
    let syn = crc24(msg, nbits);
    match df {
        17 | 18 => {
            if syn == 0 {
                1400
            } else {
                -2
            }
        }
        11 => score_df11(msg, syn, known),
        0 | 4 | 5 | 16 | 20 | 21 => {
            if known.contains(&syn) {
                1000
            } else {
                -1
            }
        }
        _ => -2,
    }
}

fn score_df11(msg: &[u8], syn: u32, known: &HashSet<u32>) -> i32 {
    let addr = icao_from_aa(msg);
    if df11_syndrome_iid_ok(syn) {
        let iid = syn & 0x7F;
        return if iid == 0 {
            if known.contains(&addr) {
                1600
            } else {
                750
            }
        } else if known.contains(&addr) {
            1000
        } else {
            -1
        };
    }
    // 1-bit repair only when corrected AA is already known (readsb behaviour).
    let mut score = -2;
    df11_for_each_1bit_repair(msg, |_trial, addr2| {
        if known.contains(&addr2) {
            score = 800;
            true
        } else {
            false
        }
    });
    score
}

fn learn_icao(msg: &[u8], nbits: usize, known: &mut HashSet<u32>) {
    let df = msg[0] >> 3;
    let syn = crc24(msg, nbits);
    match df {
        11 => {
            if df11_syndrome_iid_ok(syn) {
                known.insert(icao_from_aa(msg));
            }
        }
        17 | 18 => {
            known.insert(icao_from_aa(msg));
        }
        0 | 4 | 5 | 16 | 20 | 21 => {
            known.insert(syn);
        }
        _ => {}
    }
}

#[inline]
fn preamble_gate(p: &[u16]) -> bool {
    p[1] > p[7] && p[12] > p[14] && p[12] > p[15]
}

/// Stateful demodulator so ICAO learning persists across magnitude chunks.
#[derive(Debug, Default)]
pub struct Demodulator {
    known: HashSet<u32>,
}

impl Demodulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Scan `mag[start_j..search_end)` for preambles; decode may read through `mag.len()`.
    ///
    /// `sample_base` is the global sample index of `mag[0]` (for CPR timestamps).
    /// Returns emitted frames and the scan cursor after this window (may exceed
    /// `search_end` when a message skip lands in the overlap tail).
    pub fn process(
        &mut self,
        mag: &[u16],
        sample_base: usize,
        start_j: usize,
        search_end: usize,
    ) -> (Vec<RawFrame>, usize) {
        let mut out = Vec::new();
        let search_end = search_end.min(mag.len().saturating_sub(DEMOD_OVERLAP));
        if start_j >= search_end {
            return (out, start_j);
        }
        let mut j = start_j;
        while j < search_end {
            let p = &mag[j..];
            if !preamble_gate(p) {
                j += 1;
                continue;
            }

            let base_noise = p[5] as i32 + p[8] as i32 + p[16] as i32 + p[17] as i32 + p[18] as i32;
            let ref_level = (base_noise * PREAMBLE_THRESHOLD) >> 5;

            let diff_2_3 = p[2] as i32 - p[3] as i32;
            let sum_1_4 = p[1] as i32 + p[4] as i32;
            let diff_10_11 = p[10] as i32 - p[11] as i32;
            let common3456 = sum_1_4 - diff_2_3 + p[9] as i32 + p[12] as i32;

            let mut best: Option<([u8; 14], usize, i32)> = None;
            let mut try_phases = [false; 5];

            let pa_mag_a = common3456 - diff_10_11;
            if pa_mag_a >= ref_level {
                try_phases[0] = true;
                try_phases[1] = true;
            }
            let pa_mag_b = common3456 + diff_10_11;
            if pa_mag_b >= ref_level {
                try_phases[2] = true;
                try_phases[3] = true;
            }
            let pa_mag_c = sum_1_4 + 2 * diff_2_3 + diff_10_11 + p[12] as i32;
            if pa_mag_c >= ref_level {
                try_phases[4] = true;
            }

            if !try_phases.iter().any(|&t| t) {
                j += 1;
                continue;
            }

            for (i, &ok) in try_phases.iter().enumerate() {
                if !ok {
                    continue;
                }
                let try_phase = 4 + i;
                if let Some((msg, nbits)) = slice_message(mag, j, try_phase) {
                    let sc = score(&msg, nbits, &self.known);
                    if sc > 0 && best.as_ref().map(|(_, _, s)| sc > *s).unwrap_or(true) {
                        best = Some((msg, nbits, sc));
                    }
                }
            }

            if let Some((msg, nbits, _)) = best {
                learn_icao(&msg, nbits, &mut self.known);
                out.push((sample_base + j, msg, nbits));
                j += (nbits * 2).max(1);
            } else {
                j += 1;
            }
        }
        (out, j)
    }
}

/// One-shot demod over a full magnitude buffer (tests / simple callers).
#[allow(dead_code)] // exercised by unit tests; CLI uses the chunked pipeline
pub fn demodulate(mag: &[u16]) -> Vec<RawFrame> {
    if mag.len() < DEMOD_OVERLAP {
        return Vec::new();
    }
    let mut demod = Demodulator::new();
    let (frames, _) = demod.process(mag, 0, 0, mag.len() - DEMOD_OVERLAP);
    frames
}

/// Chunked demod over an existing magnitude buffer (parity helper / tests).
///
/// Walks `mag` in windows of `chunk_samples` with [`DEMOD_OVERLAP`] carry so
/// results match [`demodulate`] while exercising the streaming path.
#[allow(dead_code)] // unit-test parity helper
pub fn demodulate_chunked(mag: &[u16], chunk_samples: usize) -> Vec<RawFrame> {
    assert!(chunk_samples > 0);
    let mut demod = Demodulator::new();
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut start_j = 0usize;

    while pos < mag.len() {
        let end = (pos + chunk_samples + DEMOD_OVERLAP).min(mag.len());
        let window = &mag[pos..end];
        if window.len() < DEMOD_OVERLAP {
            break;
        }
        let more_coming = end < mag.len();
        let search_end = window.len() - DEMOD_OVERLAP;
        let (frames, end_j) = demod.process(window, pos, start_j, search_end);
        out.extend(frames);
        if !more_coming {
            break;
        }
        let keep_from = window.len() - DEMOD_OVERLAP;
        start_j = end_j - keep_from;
        pos += keep_from;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked_matches_oneshot_on_emptyish_mag() {
        let mag = vec![0u16; 50_000];
        assert_eq!(demodulate(&mag), demodulate_chunked(&mag, 8_192));
        assert_eq!(demodulate(&mag), demodulate_chunked(&mag, 1_024));
    }

    #[test]
    fn chunked_matches_oneshot_on_noisy_mag() {
        let mut mag = vec![0u16; 80_000];
        for (i, s) in mag.iter_mut().enumerate() {
            *s = ((i * 17 + 91) % 500) as u16;
        }
        let full = demodulate(&mag);
        let chunked = demodulate_chunked(&mag, 4096);
        assert_eq!(full, chunked);
    }
}
