//! ADS-B ME field decode: altitude, velocity, CPR, callsign (HLR-ADS-02).

use crate::cpr;
use crate::frame::DecodedFrame;
use crate::mode_s::{df, extract_icao, icao_hex};
use std::collections::{HashMap, VecDeque};

/// Preferred airborne even/odd CPR window (ICAO).
const CPR_AIRBORNE_TIMEOUT_S: f64 = 10.0;
/// Soft airborne window when no pair exists inside the preferred timeout.
/// Soft hits require a sanity reference (last fix or `--ref-*`) and a
/// ≤180 NM check — never accepted on bare cold start.
const CPR_AIRBORNE_SOFT_TIMEOUT_S: f64 = 20.0;
/// Surface CPR window (seconds); ICAO allows longer than airborne.
const CPR_SURFACE_TIMEOUT_S: f64 = 25.0;
/// Keep a few opposite-parity samples so the newest frame can pick a close pair.
const CPR_HISTORY: usize = 4;
/// Reject soft/global/local fixes that jump farther than this from a known reference.
const CPR_SANITY_NM: f64 = 180.0;

/// AIS 6-bit charset (ICAO Doc 9871 / dump1090).
const AIS_CHARSET: &[u8; 64] =
    b"@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_ !\"#$%&'()*+,-./0123456789:;<=>?";

#[derive(Clone, Copy, Debug)]
struct CprSample {
    lat_cpr: u32,
    lon_cpr: u32,
    ts: f64,
}

#[derive(Clone, Debug, Default)]
struct CprPair {
    even: VecDeque<CprSample>,
    odd: VecDeque<CprSample>,
}

#[derive(Clone, Debug, Default)]
struct AircraftState {
    airborne: CprPair,
    surface: CprPair,
    /// Last decoded position — reference for local / surface CPR.
    last_lat: Option<f64>,
    last_lon: Option<f64>,
    callsign: Option<String>,
}

/// Per-ICAO even/odd CPR cache + last fix for surface reference.
#[derive(Default)]
pub struct AdsbDecoder {
    ac: HashMap<u32, AircraftState>,
    /// Optional receiver / scenario reference for surface CPR and soft-window sanity.
    pub ref_lat: Option<f64>,
    pub ref_lon: Option<f64>,
}

impl AdsbDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_reference(lat: f64, lon: f64) -> Self {
        Self {
            ref_lat: Some(lat),
            ref_lon: Some(lon),
            ..Self::default()
        }
    }

    /// Decode one Mode S frame. ME fields are filled only for DF17/18;
    /// other DFs return a sparse frame (ICAO/DF/ts) for `--all-df` emission.
    pub fn decode(&mut self, msg: &[u8], nbits: usize, ts: f64) -> Option<DecodedFrame> {
        let d = df(msg);
        let icao = extract_icao(msg, nbits);
        let mut frame = DecodedFrame {
            icao: icao_hex(icao),
            df: d,
            tc: None,
            callsign: None,
            alt: None,
            lat: None,
            lon: None,
            gs: None,
            track: None,
            ts: Some(ts),
        };

        if d != 17 && d != 18 {
            return Some(frame);
        }
        if msg.len() < 14 || nbits < 112 {
            // Truncated DF17/18 — refuse rather than invent fields.
            return None;
        }

        let tc = msg[4] >> 3;
        frame.tc = Some(tc);

        match tc {
            1..=4 => {
                if let Some(cs) = decode_callsign(msg) {
                    let st = self.ac.entry(icao).or_default();
                    st.callsign = Some(cs.clone());
                    frame.callsign = Some(cs);
                }
            }
            5..=8 => {
                let (odd, lat_cpr, lon_cpr) = parse_cpr_airborne_layout(msg);
                let (gs, track) = decode_surface_movement(msg);
                frame.gs = gs;
                frame.track = track;
                if let Some((lat, lon)) = self.stash_surface_cpr(icao, odd, lat_cpr, lon_cpr, ts) {
                    frame.lat = Some(lat);
                    frame.lon = Some(lon);
                }
            }
            9..=18 => {
                frame.alt = decode_ac12_altitude(msg);
                let (odd, lat_cpr, lon_cpr) = parse_cpr_airborne_layout(msg);
                if let Some((lat, lon)) = self.stash_airborne_cpr(icao, odd, lat_cpr, lon_cpr, ts) {
                    frame.lat = Some(lat);
                    frame.lon = Some(lon);
                }
            }
            19 => {
                let (gs, track) = decode_velocity(msg);
                frame.gs = gs;
                frame.track = track;
            }
            20..=22 => {
                // GNSS height uses the same AC12 packing when Q=1.
                frame.alt = decode_ac12_altitude(msg);
                let (odd, lat_cpr, lon_cpr) = parse_cpr_airborne_layout(msg);
                if let Some((lat, lon)) = self.stash_airborne_cpr(icao, odd, lat_cpr, lon_cpr, ts) {
                    frame.lat = Some(lat);
                    frame.lon = Some(lon);
                }
            }
            _ => {}
        }

        if frame.callsign.is_none() {
            if let Some(st) = self.ac.get(&icao) {
                frame.callsign = st.callsign.clone();
            }
        }
        Some(frame)
    }

    fn stash_airborne_cpr(
        &mut self,
        icao: u32,
        odd: bool,
        lat_cpr: u32,
        lon_cpr: u32,
        ts: f64,
    ) -> Option<(f64, f64)> {
        let rx_lat = self.ref_lat;
        let rx_lon = self.ref_lon;
        let st = self.ac.entry(icao).or_default();
        let sample = CprSample {
            lat_cpr,
            lon_cpr,
            ts,
        };
        if odd {
            push_cpr(&mut st.airborne.odd, sample);
        } else {
            push_cpr(&mut st.airborne.even, sample);
        }

        let sanity_lat = st.last_lat.or(rx_lat);
        let sanity_lon = st.last_lon.or(rx_lon);

        // Prefer local CPR once we have a track fix (avoids replaying a stale global pair).
        if let (Some(ref_lat), Some(ref_lon)) = (st.last_lat, st.last_lon) {
            if let Some((lat, lon)) =
                cpr::decode_airborne_local(lat_cpr, lon_cpr, odd, ref_lat, ref_lon)
            {
                st.last_lat = Some(lat);
                st.last_lon = Some(lon);
                return Some((lat, lon));
            }
            if let Some((lat, lon)) = try_global_airborne_newest(
                &st.airborne.even,
                &st.airborne.odd,
                odd,
                sanity_lat,
                sanity_lon,
            ) {
                st.last_lat = Some(lat);
                st.last_lon = Some(lon);
                return Some((lat, lon));
            }
            return None;
        }

        if let Some((lat, lon)) = try_global_airborne_newest(
            &st.airborne.even,
            &st.airborne.odd,
            odd,
            sanity_lat,
            sanity_lon,
        ) {
            st.last_lat = Some(lat);
            st.last_lon = Some(lon);
            return Some((lat, lon));
        }
        None
    }

    fn stash_surface_cpr(
        &mut self,
        icao: u32,
        odd: bool,
        lat_cpr: u32,
        lon_cpr: u32,
        ts: f64,
    ) -> Option<(f64, f64)> {
        let st = self.ac.entry(icao).or_default();
        let sample = CprSample {
            lat_cpr,
            lon_cpr,
            ts,
        };
        if odd {
            push_cpr(&mut st.surface.odd, sample);
        } else {
            push_cpr(&mut st.surface.even, sample);
        }
        let (newest, opposites) = if odd {
            (st.surface.odd.back()?, &st.surface.even)
        } else {
            (st.surface.even.back()?, &st.surface.odd)
        };
        let mut best: Option<(f64, CprSample, CprSample)> = None;
        for opp in opposites {
            let dt = (newest.ts - opp.ts).abs();
            if dt > CPR_SURFACE_TIMEOUT_S {
                continue;
            }
            let (e, o) = if odd {
                (*opp, *newest)
            } else {
                (*newest, *opp)
            };
            if best.map(|(bdt, _, _)| dt < bdt).unwrap_or(true) {
                best = Some((dt, e, o));
            }
        }
        let (_, e, o) = best?;
        let even_newer = e.ts >= o.ts;
        let (ref_lat, ref_lon) = match (st.last_lat, st.last_lon) {
            (Some(a), Some(b)) => (a, b),
            _ => (self.ref_lat?, self.ref_lon?),
        };
        let (lat, lon) = cpr::decode_surface(
            e.lat_cpr, e.lon_cpr, o.lat_cpr, o.lon_cpr, even_newer, ref_lat, ref_lon,
        )?;
        st.last_lat = Some(lat);
        st.last_lon = Some(lon);
        Some((lat, lon))
    }
}

fn push_cpr(q: &mut VecDeque<CprSample>, sample: CprSample) {
    if q.len() >= CPR_HISTORY {
        q.pop_front();
    }
    q.push_back(sample);
}

/// Global airborne CPR using only the newest sample vs opposite-parity history.
///
/// - Preferred ≤10 s: allowed without a ref (cold start); sanity-checked when a ref exists.
/// - Soft ≤20 s: requires a sanity ref (last fix or `--ref-*`) and ≤180 NM.
fn try_global_airborne_newest(
    even: &VecDeque<CprSample>,
    odd: &VecDeque<CprSample>,
    new_is_odd: bool,
    ref_lat: Option<f64>,
    ref_lon: Option<f64>,
) -> Option<(f64, f64)> {
    let newest = if new_is_odd {
        odd.back()?
    } else {
        even.back()?
    };
    let opposites = if new_is_odd { even } else { odd };

    let mut pairs: Vec<(f64, CprSample, CprSample)> = Vec::new();
    for opp in opposites {
        let dt = (newest.ts - opp.ts).abs();
        if dt <= CPR_AIRBORNE_SOFT_TIMEOUT_S {
            let (e, o) = if new_is_odd {
                (*opp, *newest)
            } else {
                (*newest, *opp)
            };
            pairs.push((dt, e, o));
        }
    }
    pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut soft_hit: Option<(f64, f64)> = None;
    for (dt, e, o) in pairs {
        let even_newer = e.ts >= o.ts;
        let Some((lat, lon)) =
            cpr::decode_airborne(e.lat_cpr, e.lon_cpr, o.lat_cpr, o.lon_cpr, even_newer)
        else {
            continue;
        };
        if dt <= CPR_AIRBORNE_TIMEOUT_S {
            if preferred_global_ok(lat, lon, ref_lat, ref_lon) {
                return Some((lat, lon));
            }
        } else if soft_hit.is_none() && soft_global_ok(lat, lon, ref_lat, ref_lon) {
            soft_hit = Some((lat, lon));
        }
    }
    soft_hit
}

fn preferred_global_ok(lat: f64, lon: f64, ref_lat: Option<f64>, ref_lon: Option<f64>) -> bool {
    match (ref_lat, ref_lon) {
        (Some(rlat), Some(rlon)) => cpr::haversine_nm(lat, lon, rlat, rlon) <= CPR_SANITY_NM,
        _ => true,
    }
}

fn soft_global_ok(lat: f64, lon: f64, ref_lat: Option<f64>, ref_lon: Option<f64>) -> bool {
    match (ref_lat, ref_lon) {
        (Some(rlat), Some(rlon)) => cpr::haversine_nm(lat, lon, rlat, rlon) <= CPR_SANITY_NM,
        _ => false,
    }
}

/// Decode TC1–4 aircraft identification → callsign (trimmed).
fn decode_callsign(msg: &[u8]) -> Option<String> {
    if msg.len() < 11 {
        return None;
    }
    let mut acc: u64 = 0;
    for &b in &msg[5..11] {
        acc = (acc << 8) | u64::from(b);
    }
    let mut cs = String::with_capacity(8);
    for i in 0..8 {
        let idx = ((acc >> (42 - i * 6)) & 0x3F) as usize;
        let ch = AIS_CHARSET[idx] as char;
        // '@' is padding / unused in AIS callsign encoding.
        if ch == '@' {
            cs.push(' ');
        } else {
            cs.push(ch);
        }
    }
    let trimmed = cs.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Airborne/surface position ME CPR layout (F + LAT17 + LON17).
fn parse_cpr_airborne_layout(msg: &[u8]) -> (bool, u32, u32) {
    let f_odd = (msg[6] & 0x04) != 0;
    let lat_cpr =
        (((msg[6] as u32) & 0x03) << 15) | ((msg[7] as u32) << 7) | ((msg[8] as u32) >> 1);
    let lon_cpr = (((msg[8] as u32) & 0x01) << 16) | ((msg[9] as u32) << 8) | msg[10] as u32;
    (f_odd, lat_cpr & 0x1_FFFF, lon_cpr & 0x1_FFFF)
}

/// Decode 12-bit AC altitude field from airborne position ME → feet.
fn decode_ac12_altitude(msg: &[u8]) -> Option<i32> {
    let alt_code = (((msg[5] as u16) << 4) | ((msg[6] as u16) >> 4)) & 0x0FFF;
    if alt_code == 0 {
        return None;
    }
    let q = (alt_code & 0x10) != 0;
    if q {
        let n = ((alt_code & 0x0FE0) >> 1) | (alt_code & 0x0F);
        Some(i32::from(n) * 25 - 1000)
    } else {
        // Gillham / 100 ft — uncommon on modern ADS-B; skip rather than mis-decode.
        None
    }
}

fn decode_velocity(msg: &[u8]) -> (Option<f64>, Option<f64>) {
    let subtype = msg[4] & 0x07;
    match subtype {
        1 | 2 => decode_velocity_ground_speed(msg, subtype),
        3 | 4 => decode_velocity_airspeed(msg),
        _ => (None, None),
    }
}

fn decode_velocity_ground_speed(msg: &[u8], subtype: u8) -> (Option<f64>, Option<f64>) {
    let ew_dir = (msg[5] >> 2) & 0x01;
    let ew = (((msg[5] as u16) & 0x03) << 8) | msg[6] as u16;
    let ns_dir = (msg[7] >> 7) & 0x01;
    let ns = (((msg[7] as u16) & 0x7F) << 3) | ((msg[8] as u16) >> 5);
    if ew == 0 || ns == 0 {
        return (None, None);
    }
    let mut vew = f64::from(ew as i32 - 1);
    let mut vns = f64::from(ns as i32 - 1);
    if subtype == 2 {
        vew *= 4.0;
        vns *= 4.0;
    }
    if ew_dir == 1 {
        vew = -vew;
    }
    if ns_dir == 1 {
        vns = -vns;
    }
    let gs = (vew * vew + vns * vns).sqrt();
    let mut track = vew.atan2(vns).to_degrees();
    if track < 0.0 {
        track += 360.0;
    }
    (Some(gs), Some(track))
}

fn decode_velocity_airspeed(msg: &[u8]) -> (Option<f64>, Option<f64>) {
    let hdg_status = (msg[5] >> 2) & 0x01;
    let hdg_raw = (((msg[5] as u16) & 0x03) << 8) | msg[6] as u16;
    let as_type = (msg[7] >> 7) & 0x01; // 0=IAS, 1=TAS — report either as gs proxy
    let as_raw = (((msg[7] as u16) & 0x7F) << 3) | ((msg[8] as u16) >> 5);
    let _ = as_type;
    if as_raw == 0 {
        return (None, None);
    }
    let speed = f64::from(as_raw as i32 - 1);
    let track = if hdg_status == 1 {
        Some(hdg_raw as f64 * 360.0 / 1024.0)
    } else {
        None
    };
    (Some(speed), track)
}

/// Surface position: movement → ground speed kn; ground track when available.
fn decode_surface_movement(msg: &[u8]) -> (Option<f64>, Option<f64>) {
    let movement = ((msg[4] & 0x07) << 4) | ((msg[5] >> 4) & 0x0F);
    let gs = surface_movement_to_knots(movement);

    let track_valid = (msg[5] & 0x08) != 0;
    let track = if track_valid {
        let raw = (((msg[5] as u16) & 0x07) << 4) | ((msg[6] as u16) >> 4);
        Some(raw as f64 * 360.0 / 128.0)
    } else {
        None
    };
    (gs, track)
}

fn surface_movement_to_knots(m: u8) -> Option<f64> {
    // ICAO Doc 9871 Table 2-28 (simplified).
    match m {
        0 => None,
        1 => Some(0.0),
        2..=8 => Some(0.125 + f64::from(m - 2) * 0.125),
        9..=12 => Some(1.0 + f64::from(m - 9) * 0.25),
        13..=38 => Some(2.0 + f64::from(m - 13) * 0.5),
        39..=93 => Some(15.0 + f64::from(m - 39) * 1.0),
        94..=108 => Some(70.0 + f64::from(m - 94) * 2.0),
        109..=123 => Some(100.0 + f64::from(m - 109) * 5.0),
        124 => Some(175.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_msg(s: &str) -> [u8; 14] {
        let mut out = [0u8; 14];
        for i in 0..14 {
            out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
        }
        out
    }

    #[test]
    fn ac12_altitude_25ft() {
        // alt = n*25-1000 = 36000 → n=1480; Q-bit packing into ME bytes 5-6.
        let n: u16 = 1480;
        let alt_code = ((n & 0x07F0) << 1) | 0x10 | (n & 0x0F);
        let mut msg = [0u8; 14];
        msg[5] = (alt_code >> 4) as u8;
        msg[6] = ((alt_code & 0x0F) << 4) as u8;
        assert_eq!(decode_ac12_altitude(&msg), Some(36_000));
    }

    #[test]
    fn df17_velocity_groundspeed() {
        // Captured DF17: *8d4ca92d99154f1700bcb3b027d7
        let msg = hex_msg("8d4ca92d99154f1700bcb3b027d7");
        let mut dec = AdsbDecoder::new();
        let f = dec.decode(&msg, 112, 0.0).unwrap();
        assert_eq!(f.df, 17);
        assert_eq!(f.tc, Some(19));
        assert!(f.gs.unwrap() > 380.0 && f.gs.unwrap() < 382.0);
        assert!(f.track.unwrap() > 298.0 && f.track.unwrap() < 300.0);
    }

    #[test]
    fn df17_airborne_cpr_pair() {
        // Classic even/odd pair (ICAO 40621D) → ~52.257 / 3.919
        let even = hex_msg("8d40621d58c382d690c8ac2863a7");
        let odd = hex_msg("8d40621d58c386435cc412692ad6");
        let mut dec = AdsbDecoder::new();
        // Odd first, then even (newer) — published coords use even-frame solution.
        assert!(dec.decode(&odd, 112, 1.0).unwrap().lat.is_none());
        let f1 = dec.decode(&even, 112, 1.5).unwrap();
        let lat = f1.lat.expect("lat");
        let lon = f1.lon.expect("lon");
        assert!((lat - 52.257).abs() < 0.01, "lat={lat}");
        assert!((lon - 3.919).abs() < 0.01, "lon={lon}");
        assert!(f1.alt.is_some());
    }

    #[test]
    fn df17_callsign() {
        let msg = hex_msg("8d4ca92b234994b5cd3360d23b80");
        let mut dec = AdsbDecoder::new();
        let f = dec.decode(&msg, 112, 0.0).unwrap();
        assert_eq!(f.tc, Some(4));
        assert_eq!(f.callsign.as_deref(), Some("RYR53SM"));
    }

    #[test]
    fn airborne_cpr_preferred_window_cold_start() {
        let odd = hex_msg("8d40621d58c386435cc412692ad6");
        let even = hex_msg("8d40621d58c382d690c8ac2863a7");
        let mut dec = AdsbDecoder::new();
        assert!(dec.decode(&odd, 112, 0.0).unwrap().lat.is_none());
        let f = dec.decode(&even, 112, 5.0).unwrap();
        assert!(f.lat.is_some());
        assert!((f.lat.unwrap() - 52.257).abs() < 0.01);
    }

    #[test]
    fn airborne_cpr_soft_window_rejected_without_ref() {
        let odd = hex_msg("8d40621d58c386435cc412692ad6");
        let even = hex_msg("8d40621d58c382d690c8ac2863a7");
        let mut dec = AdsbDecoder::new();
        assert!(dec.decode(&odd, 112, 0.0).unwrap().lat.is_none());
        let f = dec.decode(&even, 112, 15.5).unwrap();
        assert!(f.lat.is_none(), "soft without ref must reject");
    }

    #[test]
    fn airborne_cpr_soft_window_accepted_with_ref() {
        let odd = hex_msg("8d40621d58c386435cc412692ad6");
        let even = hex_msg("8d40621d58c382d690c8ac2863a7");
        let mut dec = AdsbDecoder::with_reference(52.3, 4.0);
        assert!(dec.decode(&odd, 112, 0.0).unwrap().lat.is_none());
        let f = dec.decode(&even, 112, 15.5).unwrap();
        assert!(f.lat.is_some(), "soft with nearby ref should decode");
        assert!((f.lat.unwrap() - 52.257).abs() < 0.01);
    }

    #[test]
    fn airborne_cpr_soft_window_rejected_far_ref() {
        let odd = hex_msg("8d40621d58c386435cc412692ad6");
        let even = hex_msg("8d40621d58c382d690c8ac2863a7");
        let mut dec = AdsbDecoder::with_reference(-33.9, 151.2);
        assert!(dec.decode(&odd, 112, 0.0).unwrap().lat.is_none());
        let f = dec.decode(&even, 112, 15.5).unwrap();
        assert!(f.lat.is_none(), "soft with far ref must reject");
    }

    #[test]
    fn airborne_cpr_local_after_global() {
        let even = hex_msg("8d40621d58c382d690c8ac2863a7");
        let odd = hex_msg("8d40621d58c386435cc412692ad6");
        let mut dec = AdsbDecoder::new();
        assert!(dec.decode(&odd, 112, 1.0).unwrap().lat.is_none());
        let f0 = dec.decode(&even, 112, 1.5).unwrap();
        let lat0 = f0.lat.expect("global");
        let lon0 = f0.lon.expect("global");

        let f1 = dec.decode(&even, 112, 2.0).unwrap();
        let lat1 = f1.lat.expect("local");
        let lon1 = f1.lon.expect("local");
        assert!((lat1 - lat0).abs() < 0.05, "lat0={lat0} lat1={lat1}");
        assert!((lon1 - lon0).abs() < 0.05, "lon0={lon0} lon1={lon1}");
    }

    #[test]
    fn airborne_cpr_newest_only_not_stale_all_pairs() {
        // Regression: all-pairs history used to pair a *new* even with a *stale* odd
        // while attaching the current timestamp — wrong trajectory point.
        // Setup: establish a good global fix, then push a fresh even that only
        // pairs usefully via local CPR; a far-stale odd in history must not win.
        let even = hex_msg("8d40621d58c382d690c8ac2863a7");
        let odd = hex_msg("8d40621d58c386435cc412692ad6");
        let mut dec = AdsbDecoder::new();
        dec.decode(&odd, 112, 0.0).unwrap();
        let f0 = dec.decode(&even, 112, 1.0).unwrap();
        let (lat0, lon0) = (f0.lat.unwrap(), f0.lon.unwrap());

        // Inject a second odd much later that would form a soft/stale all-pairs hit
        // with an *old* even if we replayed history; newest-only + local prefer
        // should keep the position near the last fix with the new frame's ts.
        dec.decode(&odd, 112, 50.0).unwrap(); // orphan odd (no counterpart in soft window from last even@1)
        let f1 = dec.decode(&even, 112, 51.0).unwrap();
        let lat1 = f1.lat.expect("should local-update from last fix");
        let lon1 = f1.lon.expect("lon");
        assert!(
            (lat1 - lat0).abs() < 0.05,
            "stale all-pairs would drift; lat0={lat0} lat1={lat1}"
        );
        assert!((lon1 - lon0).abs() < 0.05, "lon0={lon0} lon1={lon1}");
        assert!((f1.ts.unwrap() - 51.0).abs() < 1e-9);
    }
}
