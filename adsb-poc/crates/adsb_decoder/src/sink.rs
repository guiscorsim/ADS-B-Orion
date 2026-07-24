//! Output sink for the decoder CLI: JSONL, stats, optional track export.

use std::collections::BTreeSet;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use crate::frame::{DecodedFrame, TrackBuffer};

/// Aggregate counters for `--stats` (mission-oriented).
#[derive(Debug, Clone, Default)]
pub struct DecodeStats {
    pub messages: u64,
    pub crc_ok: u64,
    pub df17_18: u64,
    pub icaos: BTreeSet<String>,
    pub icaos_with_pos: BTreeSet<String>,
    pub icaos_with_alt: BTreeSet<String>,
    pub icaos_with_vel: BTreeSet<String>,
    pub icaos_with_callsign: BTreeSet<String>,
}

impl DecodeStats {
    pub fn record(&mut self, frame: &DecodedFrame, crc_ok: bool) {
        self.messages += 1;
        if crc_ok {
            self.crc_ok += 1;
        }
        if !frame.icao.is_empty() {
            self.icaos.insert(frame.icao.clone());
        }
        if frame.df == 17 || frame.df == 18 {
            self.df17_18 += 1;
            if frame.lat.is_some() && frame.lon.is_some() {
                self.icaos_with_pos.insert(frame.icao.clone());
            }
            if frame.alt.is_some() {
                self.icaos_with_alt.insert(frame.icao.clone());
            }
            if frame.gs.is_some() {
                self.icaos_with_vel.insert(frame.icao.clone());
            }
            if frame.callsign.is_some() {
                self.icaos_with_callsign.insert(frame.icao.clone());
            }
        }
    }

    pub fn unique_icaos(&self) -> usize {
        self.icaos.len()
    }

    pub fn print_summary(&self, w: &mut dyn Write) -> io::Result<()> {
        writeln!(w, "messages: {}", self.messages)?;
        writeln!(w, "crc_ok: {}", self.crc_ok)?;
        writeln!(w, "df17_18: {}", self.df17_18)?;
        writeln!(w, "unique_icao: {}", self.unique_icaos())?;
        writeln!(w, "icao_with_position: {}", self.icaos_with_pos.len())?;
        writeln!(w, "icao_with_altitude: {}", self.icaos_with_alt.len())?;
        writeln!(w, "icao_with_velocity: {}", self.icaos_with_vel.len())?;
        writeln!(w, "icao_with_callsign: {}", self.icaos_with_callsign.len())?;
        Ok(())
    }
}

/// Writes one JSON object per line (bench schema).
pub struct JsonlSink<W: Write> {
    writer: BufWriter<W>,
}

impl<W: Write> JsonlSink<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer: BufWriter::new(writer),
        }
    }

    pub fn emit(&mut self, frame: &DecodedFrame) -> io::Result<()> {
        serde_json::to_writer(&mut self.writer, frame)?;
        self.writer.write_all(b"\n")?;
        Ok(())
    }

    pub fn finish(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl JsonlSink<std::fs::File> {
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let f = std::fs::File::create(path)?;
        Ok(Self::new(f))
    }
}

/// Fan-out sink: JSONL (optional) + always-on stats + optional tracks.
///
/// Caller gates which frames are emitted; `crc_ok` is recorded for real integrity
/// (DF11 with non-IID residual counts as fail even when `--all-df` emits it).
pub struct BenchSink {
    jsonl: Option<JsonlSink<std::fs::File>>,
    tracks_path: Option<std::path::PathBuf>,
    tracks: TrackBuffer,
    pub stats: DecodeStats,
}

impl BenchSink {
    pub fn new(jsonl_path: Option<&Path>, tracks_path: Option<&Path>) -> io::Result<Self> {
        let jsonl = match jsonl_path {
            Some(p) => Some(JsonlSink::create(p)?),
            None => None,
        };
        Ok(Self {
            jsonl,
            tracks_path: tracks_path.map(|p| p.to_path_buf()),
            tracks: TrackBuffer::new(),
            stats: DecodeStats::default(),
        })
    }

    /// Emit an accepted frame; `crc_ok` reflects [`crate::mode_s::message_crc_ok`].
    pub fn emit(&mut self, frame: &DecodedFrame, crc_ok: bool) -> io::Result<()> {
        self.stats.record(frame, crc_ok);
        if self.tracks_path.is_some() {
            self.tracks.observe(frame);
        }
        if let Some(s) = self.jsonl.as_mut() {
            s.emit(frame)?;
        }
        Ok(())
    }

    pub fn finish(&mut self) -> io::Result<()> {
        if let Some(s) = self.jsonl.as_mut() {
            s.finish()?;
        }
        if let Some(path) = self.tracks_path.as_ref() {
            self.tracks.write_to_path(path)?;
        }
        Ok(())
    }
}
