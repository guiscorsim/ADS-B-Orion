//! Mode S framing: DF, ICAO, CRC-24, DF11 repair.
//!
//! Demod still accepts DF11 (and known-ICAO surv replies) for address learning.
//! Product output defaults to DF17/18 only (see CLI `--all-df`).

/// Mode S CRC-24 over the full message (including PI). Valid DF17 → 0.
pub fn crc24(msg: &[u8], nbits: usize) -> u32 {
    let nbytes = nbits / 8;
    let mut crc = 0u32;
    for &byte in &msg[..nbytes] {
        crc ^= (byte as u32) << 16;
        for _ in 0..8 {
            crc <<= 1;
            if (crc & 0x100_0000) != 0 {
                crc ^= 0xFF_F409;
            }
        }
    }
    crc & 0xFF_FFFF
}

pub fn df(msg: &[u8]) -> u8 {
    msg[0] >> 3
}

pub fn icao_from_aa(msg: &[u8]) -> u32 {
    ((msg[1] as u32) << 16) | ((msg[2] as u32) << 8) | msg[3] as u32
}

pub fn icao_hex(addr: u32) -> String {
    format!("{:06x}", addr & 0xFF_FFFF)
}

/// True when DF11 CRC residual is only the 7-bit interrogator ID.
#[inline]
pub fn df11_syndrome_iid_ok(syn: u32) -> bool {
    (syn & 0xFFFF80) == 0
}

/// Call `f(repaired_msg, aa)` for each 1-bit DF11 repair with a clean IID residual.
/// Stops early when `f` returns `true`.
pub fn df11_for_each_1bit_repair(msg: &[u8], mut f: impl FnMut(&[u8; 7], u32) -> bool) {
    if msg.len() < 7 {
        return;
    }
    for bit in 0..56usize {
        let mut trial = [0u8; 7];
        trial.copy_from_slice(&msg[..7]);
        trial[bit / 8] ^= 1 << (7 - (bit % 8));
        let syn2 = crc24(&trial, 56);
        if df11_syndrome_iid_ok(syn2) {
            let aa = icao_from_aa(&trial);
            if f(&trial, aa) {
                return;
            }
        }
    }
}

/// Integrity check for stats: DF17/18 residual 0; DF11 IID-only; surv DFs accepted
/// upstream via known-ICAO address match (PI == ICAO).
pub fn message_crc_ok(msg: &[u8], nbits: usize) -> bool {
    let syn = crc24(msg, nbits);
    match df(msg) {
        17 | 18 => syn == 0,
        11 => df11_syndrome_iid_ok(syn),
        0 | 4 | 5 | 16 | 20 | 21 => true,
        _ => false,
    }
}

/// Accept DF17/18 (CRC 0), DF11 (IID residual, possibly after demod 1-bit
/// repair), or DF0/4/5/16/20/21 when demod already matched a known ICAO.
pub fn accept_message(msg: &[u8], nbits: usize) -> bool {
    let d = df(msg);
    let syn = crc24(msg, nbits);
    match d {
        17 | 18 => syn == 0,
        11 => true, // demod scored DF11 (clean or 1-bit-repaired)
        0 | 4 | 5 | 16 | 20 | 21 => true,
        _ => false,
    }
}

pub fn extract_icao(msg: &[u8], nbits: usize) -> u32 {
    match df(msg) {
        11 => df11_addr(msg),
        17 | 18 => icao_from_aa(msg),
        0 | 4 | 5 | 16 | 20 | 21 => crc24(msg, nbits),
        _ => icao_from_aa(msg),
    }
}

fn df11_addr(msg: &[u8]) -> u32 {
    let syn = crc24(msg, 56);
    if df11_syndrome_iid_ok(syn) {
        return icao_from_aa(msg);
    }
    let mut found = None;
    df11_for_each_1bit_repair(msg, |trial, _aa| {
        found = Some(icao_from_aa(trial));
        true
    });
    found.unwrap_or_else(|| icao_from_aa(msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn df17_known_crc_zero() {
        let msg = [
            0x8c, 0x48, 0x41, 0x75, 0x3a, 0x9a, 0x15, 0x32, 0x37, 0xae, 0xf0, 0xf2, 0x75, 0xbe,
        ];
        assert_eq!(df(&msg), 17);
        assert_eq!(crc24(&msg, 112), 0);
        assert_eq!(icao_from_aa(&msg), 0x484175);
        assert!(message_crc_ok(&msg, 112));
    }
}
