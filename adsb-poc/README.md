# adsb-poc

A small Rust decoder (`adsb_decoder`) that turns IQ captures into ADS-B DF17/18 messages, compared against [readsb](https://github.com/wiedehopf/readsb). Proof of Concept built for the [CubeDesign ADS-B](adsb_mission.md) mission.

## Setup

Flakes required. Then `direnv allow` or `nix develop`.

## Run

```bash
cargo build --release

cargo run -p adsb_decoder --release -- \
  --ifile bench/sample.sc16 --iformat sc16 --stats --jsonl out.jsonl
```

Useful extras: `--tracks PATH`, `--all-df`, `--ref-lat` / `--ref-lon` (soft CPR). Full list: `--help`.

## Fixture + bench

Drop stereo IQ at `bench/sample.wav` (gitignored), then:

```bash
./bench/prepare.sh   # → bench/sample.sc16 @ 2.4 MHz s16le stereo
./bench/run.sh       # readsb vs adsb_decoder → bench/out/
```

Outputs wall time and mission metrics in `bench/out/` (`summary.txt`, `summary.json`). Override paths with `SAMPLE_WAV` / `SAMPLE_SC16`; `REF_LAT` / `REF_LON` control soft CPR.