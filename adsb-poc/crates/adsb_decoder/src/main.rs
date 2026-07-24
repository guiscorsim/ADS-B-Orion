mod adsb;
mod cpr;
mod demod;
mod frame;
mod iq;
mod mode_s;
mod pipeline;
mod sink;

use std::path::PathBuf;

use clap::Parser;

use crate::adsb::AdsbDecoder;
use crate::iq::SAMPLE_RATE;
use crate::mode_s::{accept_message, df, message_crc_ok};
use crate::pipeline::demodulate_sc16_file;
use crate::sink::BenchSink;

#[derive(Debug, Parser)]
#[command(
    name = "adsb_decoder",
    about = "Hand-rolled IQ demod + ADS-B (DF17/18) decode for CubeDesign HLR-ADS"
)]
struct Args {
    /// Raw interleaved SC16 IQ file
    #[arg(long)]
    ifile: PathBuf,

    /// Input format (only sc16 supported)
    #[arg(long, default_value = "sc16")]
    iformat: String,

    /// Write decoded frames as JSONL
    #[arg(long)]
    jsonl: Option<PathBuf>,

    /// Write per-ICAO trajectories (pos/vel, time-ordered).
    /// `.jsonl` → one aircraft object per line; otherwise a JSON array.
    #[arg(long)]
    tracks: Option<PathBuf>,

    /// Print mission / ICAO stats to stderr
    #[arg(long, default_value_t = false)]
    stats: bool,

    /// Also emit non-ADS-B accepted Mode S (DF11 etc.). Default is DF17/18 only;
    /// other DFs still feed demod ICAO learning either way.
    #[arg(long, default_value_t = false)]
    all_df: bool,

    /// Optional surface / soft-CPR sanity reference latitude (degrees).
    /// Must be set together with `--ref-lon`.
    #[arg(long, allow_hyphen_values = true)]
    ref_lat: Option<f64>,

    /// Optional surface / soft-CPR sanity reference longitude (degrees).
    /// Must be set together with `--ref-lat`.
    #[arg(long, allow_hyphen_values = true)]
    ref_lon: Option<f64>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let fmt = args.iformat.to_ascii_lowercase();
    if fmt != "sc16" {
        return Err(format!("unsupported --iformat {fmt} (only sc16)").into());
    }
    match (args.ref_lat, args.ref_lon) {
        (Some(_), None) | (None, Some(_)) => {
            return Err("both --ref-lat and --ref-lon are required when either is set".into());
        }
        _ => {}
    }

    let raw_frames = demodulate_sc16_file(&args.ifile)?;

    let mut sink = BenchSink::new(args.jsonl.as_deref(), args.tracks.as_deref())?;
    let mut decoder = match (args.ref_lat, args.ref_lon) {
        (Some(lat), Some(lon)) => AdsbDecoder::with_reference(lat, lon),
        _ => AdsbDecoder::new(),
    };

    for (idx, raw, nbits) in raw_frames {
        let nbytes = nbits / 8;
        let msg = &raw[..nbytes];
        if !accept_message(msg, nbits) {
            continue;
        }
        let d = df(msg);
        if !args.all_df && d != 17 && d != 18 {
            continue;
        }
        let ts = idx as f64 / SAMPLE_RATE;
        let crc_ok = message_crc_ok(msg, nbits);
        if let Some(frame) = decoder.decode(msg, nbits, ts) {
            sink.emit(&frame, crc_ok)?;
        }
    }
    sink.finish()?;

    if args.stats {
        sink.stats.print_summary(&mut std::io::stderr())?;
    }
    Ok(())
}
