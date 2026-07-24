//! Chunked IQ→magnitude→demod pipeline (producer∥consumer).

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::mpsc::sync_channel;
use std::thread;

use crate::demod::{Demodulator, RawFrame, DEMOD_OVERLAP};
use crate::iq::magnitude_from_le_bytes;

/// Complex samples per mag chunk (~0.44 s @ 2.4 Msps, 2 MiB of `u16`).
pub const MAG_CHUNK_SAMPLES: usize = 1 << 20;

/// Bounded queue depth between mag convert and demod (readsb-style ring).
const MAG_QUEUE_DEPTH: usize = 2;

/// Read SC16 from `path` and run the chunked / pipelined demod path.
pub fn demodulate_sc16_file(path: &Path) -> std::io::Result<Vec<RawFrame>> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len() as usize;
    let total_samples = file_len / 4;
    if total_samples < DEMOD_OVERLAP {
        return Ok(Vec::new());
    }
    Ok(demodulate_sc16_reader(file, total_samples))
}

/// Demodulate an in-memory SC16 byte slice (tests).
#[cfg(test)]
pub fn demodulate_sc16_bytes(bytes: &[u8]) -> Vec<RawFrame> {
    let total_samples = bytes.len() / 4;
    if total_samples < DEMOD_OVERLAP {
        return Vec::new();
    }
    demodulate_sc16_bytes_pipelined(bytes)
}

/// Single-threaded chunked path (mag then demod per window). Useful for tests.
#[cfg(test)]
pub fn demodulate_sc16_bytes_serial(bytes: &[u8]) -> Vec<RawFrame> {
    let total_samples = bytes.len() / 4;
    if total_samples < DEMOD_OVERLAP {
        return Vec::new();
    }

    let mut demod = Demodulator::new();
    let mut out = Vec::new();
    let mut carry: Vec<u16> = Vec::with_capacity(DEMOD_OVERLAP);
    let mut sample_base = 0usize;
    let mut start_j = 0usize;
    let mut iq_sample = 0usize;

    while iq_sample < total_samples {
        let take = (total_samples - iq_sample).min(MAG_CHUNK_SAMPLES);
        let chunk_bytes = &bytes[iq_sample * 4..(iq_sample + take) * 4];
        let new_mag = magnitude_from_le_bytes(chunk_bytes);
        iq_sample += take;
        let more_coming = iq_sample < total_samples;
        consume_mag_chunk(
            &mut demod,
            &mut out,
            &mut carry,
            &mut sample_base,
            &mut start_j,
            new_mag,
            more_coming,
        );
    }
    out
}

fn demodulate_sc16_reader(mut file: File, total_samples: usize) -> Vec<RawFrame> {
    let (tx, rx) = sync_channel::<Vec<u16>>(MAG_QUEUE_DEPTH);

    thread::scope(|scope| {
        scope.spawn(move || {
            let mut iq_sample = 0usize;
            let mut raw = vec![0u8; MAG_CHUNK_SAMPLES * 4];
            while iq_sample < total_samples {
                let take = (total_samples - iq_sample).min(MAG_CHUNK_SAMPLES);
                let nbytes = take * 4;
                if let Err(e) = file.read_exact(&mut raw[..nbytes]) {
                    eprintln!("adsb_decoder: IQ read failed at sample {iq_sample}: {e}");
                    break;
                }
                let mag = magnitude_from_le_bytes(&raw[..nbytes]);
                iq_sample += take;
                if tx.send(mag).is_err() {
                    break;
                }
            }
        });

        consume_mag_stream(rx)
    })
}

#[cfg(test)]
fn demodulate_sc16_bytes_pipelined(bytes: &[u8]) -> Vec<RawFrame> {
    let total_samples = bytes.len() / 4;
    let (tx, rx) = sync_channel::<Vec<u16>>(MAG_QUEUE_DEPTH);

    thread::scope(|scope| {
        scope.spawn(move || {
            let mut iq_sample = 0usize;
            while iq_sample < total_samples {
                let take = (total_samples - iq_sample).min(MAG_CHUNK_SAMPLES);
                let chunk_bytes = &bytes[iq_sample * 4..(iq_sample + take) * 4];
                let mag = magnitude_from_le_bytes(chunk_bytes);
                iq_sample += take;
                if tx.send(mag).is_err() {
                    break;
                }
            }
        });

        consume_mag_stream(rx)
    })
}

fn consume_mag_stream(rx: std::sync::mpsc::Receiver<Vec<u16>>) -> Vec<RawFrame> {
    let mut demod = Demodulator::new();
    let mut out = Vec::new();
    let mut carry: Vec<u16> = Vec::with_capacity(DEMOD_OVERLAP);
    let mut sample_base = 0usize;
    let mut start_j = 0usize;

    // Delay one chunk so we know whether more data follows (last vs not).
    let mut pending: Option<Vec<u16>> = None;
    for mag in rx {
        if let Some(prev) = pending.take() {
            consume_mag_chunk(
                &mut demod,
                &mut out,
                &mut carry,
                &mut sample_base,
                &mut start_j,
                prev,
                true,
            );
        }
        pending = Some(mag);
    }
    if let Some(last) = pending {
        consume_mag_chunk(
            &mut demod,
            &mut out,
            &mut carry,
            &mut sample_base,
            &mut start_j,
            last,
            false,
        );
    }
    out
}

fn consume_mag_chunk(
    demod: &mut Demodulator,
    out: &mut Vec<RawFrame>,
    carry: &mut Vec<u16>,
    sample_base: &mut usize,
    start_j: &mut usize,
    new_mag: Vec<u16>,
    more_coming: bool,
) {
    let mut mag = std::mem::take(carry);
    mag.extend_from_slice(&new_mag);

    if mag.len() < DEMOD_OVERLAP {
        *carry = mag;
        return;
    }

    let search_end = mag.len() - DEMOD_OVERLAP;
    let (frames, end_j) = demod.process(&mag, *sample_base, *start_j, search_end);
    out.extend(frames);

    if more_coming {
        let keep_from = mag.len() - DEMOD_OVERLAP;
        *start_j = end_j - keep_from;
        *sample_base += keep_from;
        carry.clear();
        carry.extend_from_slice(&mag[keep_from..]);
    } else {
        carry.clear();
        *start_j = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demod::demodulate;
    use crate::iq::magnitude_from_le_bytes;

    fn synth_sc16(n_samples: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(n_samples * 4);
        for i in 0..n_samples {
            let i_s = ((i * 13) % 2000) as i16;
            let q_s = ((i * 29) % 2000) as i16 - 1000;
            bytes.extend_from_slice(&i_s.to_le_bytes());
            bytes.extend_from_slice(&q_s.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn serial_and_pipelined_match_oneshot() {
        let bytes = synth_sc16(200_000);
        let mag = magnitude_from_le_bytes(&bytes);
        let oneshot = demodulate(&mag);
        let serial = demodulate_sc16_bytes_serial(&bytes);
        let pipelined = demodulate_sc16_bytes(&bytes);
        assert_eq!(oneshot, serial);
        assert_eq!(oneshot, pipelined);
    }

    #[test]
    fn fixture_prefix_parity_when_present() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/sample.sc16");
        if !path.exists() {
            return;
        }
        let mut file = File::open(&path).unwrap();
        // ~2 s of IQ keeps the test quick but hits many chunk boundaries.
        let n_bytes = 2_400_000usize * 2 * 4;
        let mut bytes = vec![0u8; n_bytes];
        file.read_exact(&mut bytes).unwrap();
        let mag = magnitude_from_le_bytes(&bytes);
        let oneshot = demodulate(&mag);
        assert_eq!(oneshot, demodulate_sc16_bytes_serial(&bytes));
        assert_eq!(oneshot, demodulate_sc16_bytes(&bytes));
    }

    #[test]
    fn file_reader_matches_bytes_path() {
        let bytes = synth_sc16(150_000);
        let dir = std::env::temp_dir();
        let path = dir.join(format!("adsb_pipe_test_{}.sc16", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let from_file = demodulate_sc16_file(&path).unwrap();
        let from_bytes = demodulate_sc16_bytes(&bytes);
        let _ = std::fs::remove_file(&path);
        assert_eq!(from_file, from_bytes);
    }
}
