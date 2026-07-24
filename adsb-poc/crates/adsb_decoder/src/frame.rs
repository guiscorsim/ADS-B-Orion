//! Shared decoded-frame and trajectory types (bench / sink / ADS-B decode).

use std::collections::BTreeMap;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use serde::Serialize;

/// Soft per-track point cap to bound memory on long captures (0 = unlimited).
const MAX_POINTS_PER_TRACK: usize = 50_000;

/// Common decoded-frame schema used by the bench harness.
#[derive(Debug, Clone, Serialize)]
pub struct DecodedFrame {
    pub icao: String,
    pub df: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tc: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callsign: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<f64>,
}

/// One time-ordered sample on an aircraft track (HLR-ADS-07).
#[derive(Debug, Clone, Serialize)]
pub struct TrackPoint {
    pub ts: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<f64>,
}

/// Per-ICAO trajectory for offline OD / analysis (not a full OD estimator).
#[derive(Debug, Clone, Serialize)]
pub struct AircraftTrack {
    pub icao: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callsign: Option<String>,
    pub points: Vec<TrackPoint>,
}

/// Collects time-ordered pos/vel samples keyed by ICAO address.
///
/// ID-only / callsign-only frames do **not** create empty trajectories; callsign
/// is applied when a pos/vel point is recorded (or when updating an existing track).
/// Points without a timestamp are skipped. Per-track growth is capped at
/// [`MAX_POINTS_PER_TRACK`].
#[derive(Debug, Default)]
pub struct TrackBuffer {
    tracks: BTreeMap<u32, AircraftTrack>,
}

impl TrackBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a frame if it carries position and/or velocity (mission fields).
    pub fn observe(&mut self, frame: &DecodedFrame) {
        let Some(icao) = parse_icao_hex(&frame.icao) else {
            return;
        };
        let has_pos = frame.lat.is_some() && frame.lon.is_some();
        let has_vel = frame.gs.is_some();
        if !has_pos && !has_vel {
            if let Some(cs) = frame.callsign.as_ref() {
                if let Some(t) = self.tracks.get_mut(&icao) {
                    if t.callsign.is_none() {
                        t.callsign = Some(cs.clone());
                    }
                }
            }
            return;
        }
        let Some(ts) = frame.ts else {
            return;
        };
        let t = self.tracks.entry(icao).or_insert_with(|| AircraftTrack {
            icao: frame.icao.clone(),
            callsign: None,
            points: Vec::new(),
        });
        if let Some(cs) = frame.callsign.as_ref() {
            t.callsign = Some(cs.clone());
        }
        if t.points.len() >= MAX_POINTS_PER_TRACK {
            return;
        }
        t.points.push(TrackPoint {
            ts,
            lat: frame.lat,
            lon: frame.lon,
            alt: frame.alt,
            gs: frame.gs,
            track: frame.track,
        });
    }

    /// Write tracks. `.jsonl` → one aircraft JSON object per line; otherwise a JSON array.
    pub fn write_to_path(&self, path: &Path) -> io::Result<()> {
        let f = std::fs::File::create(path)?;
        let mut w = BufWriter::new(f);
        let jsonl = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("jsonl"))
            .unwrap_or(false);
        if jsonl {
            for t in self.tracks.values() {
                serde_json::to_writer(&mut w, t)?;
                w.write_all(b"\n")?;
            }
        } else {
            let list: Vec<&AircraftTrack> = self.tracks.values().collect();
            serde_json::to_writer_pretty(&mut w, &list)?;
            w.write_all(b"\n")?;
        }
        w.flush()
    }
}

fn parse_icao_hex(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    u32::from_str_radix(s, 16).ok().map(|v| v & 0xFF_FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // Keep a real extension (`name` like "tracks.json" / "tracks.jsonl").
        std::env::temp_dir().join(format!("adsb_decoder_{nanos}_{name}"))
    }

    fn frame(
        icao: &str,
        ts: Option<f64>,
        lat: Option<f64>,
        lon: Option<f64>,
        gs: Option<f64>,
    ) -> DecodedFrame {
        DecodedFrame {
            icao: icao.to_string(),
            df: 17,
            tc: Some(11),
            callsign: None,
            alt: Some(36000),
            lat,
            lon,
            gs,
            track: None,
            ts,
        }
    }

    #[test]
    fn track_buffer_skips_id_only_and_missing_ts() {
        let mut buf = TrackBuffer::new();
        let mut id = frame("abc123", Some(1.0), None, None, None);
        id.callsign = Some("TEST123".into());
        buf.observe(&id);
        assert!(buf.tracks.is_empty(), "ID-only must not create empty track");

        buf.observe(&frame("abc123", None, Some(1.0), Some(2.0), None));
        assert!(buf.tracks.is_empty(), "missing ts must not record");

        buf.observe(&frame("abc123", Some(1.5), Some(52.0), Some(4.0), None));
        assert_eq!(buf.tracks.len(), 1);
        assert_eq!(buf.tracks[&0x00abc123].points.len(), 1);
        assert!((buf.tracks[&0x00abc123].points[0].ts - 1.5).abs() < 1e-9);
    }

    #[test]
    fn track_buffer_write_json_and_jsonl() {
        let mut buf = TrackBuffer::new();
        buf.observe(&frame(
            "40621d",
            Some(1.0),
            Some(52.25),
            Some(3.92),
            Some(400.0),
        ));

        let json_path = tmp("tracks.json");
        let jsonl_path = tmp("tracks.jsonl");
        buf.write_to_path(&json_path).unwrap();
        buf.write_to_path(&jsonl_path).unwrap();

        let json = fs::read_to_string(&json_path).unwrap();
        let jsonl = fs::read_to_string(&jsonl_path).unwrap();
        let _ = fs::remove_file(&json_path);
        let _ = fs::remove_file(&jsonl_path);

        assert!(json.contains("40621d"));
        assert!(json.trim_start().starts_with('['));
        assert!(jsonl.contains("40621d"));
        assert!(jsonl.lines().count() >= 1);
        assert!(!jsonl.trim_start().starts_with('['));
    }
}
