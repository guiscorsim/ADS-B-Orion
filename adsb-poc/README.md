# adsb-poc

Nix shell with [readsb](https://github.com/wiedehopf/readsb) plus a Rust DF17/18 decoder (`adsb_decoder`) scoped to the [CubeDesign ADS-B](adsb_mission.md) mission: ICAO, position, altitude, velocity. Not full Mode S parity with readsb.

```
crates/adsb_decoder/   # IQ → DF17/18
bench/                 # fixture prep + mission-metric compare
flake.nix              # Linux: readsb, rustc/cargo, ffmpeg, hyperfine
adsb_mission.md
```

## Setup

Flakes on. With direnv: `direnv allow`. Else: `nix develop`.

## Build / run

```bash
cargo build --release
# → target/release/adsb_decoder

cargo run -p adsb_decoder --release -- \
  --ifile bench/sample.sc16 --iformat sc16 --stats --jsonl out.jsonl
```

Useful flags:

| Flag | Notes |
|------|--------|
| `--tracks PATH` | per-ICAO trajectories (`.json` array or `.jsonl`) |
| `--all-df` | emit non-DF17/18 Mode S too |
| `--ref-lat` / `--ref-lon` | together; soft CPR sanity ref near the capture scene |

Timestamps come from sample index. Default emit is DF17/18 only. Soft ≤20 s CPR needs a ref (or prior fix) and a ≤180 NM check.

## Fixture

Convention: put stereo IQ at `bench/sample.wav`, convert to interleaved SC16 (`s16le` stereo @ 2.4 MHz) as `bench/sample.sc16`. Both are gitignored — bring your own capture.

```bash
./bench/prepare.sh
# or: ffmpeg -y -i bench/sample.wav -f s16le -ac 2 -ar 2400000 bench/sample.sc16
```

Override with `SAMPLE_WAV` / `SAMPLE_SC16` if needed.

## Bench

```bash
./bench/run.sh
```

Compares readsb and `adsb_decoder` (no throttle) on `bench/sample.sc16`. Writes wall time and mission metrics under `bench/out/` (`summary.txt`, `summary.json`): DF17/18 counts, unique ICAOs, ICAOs with position / altitude / velocity.

Set `REF_LAT` / `REF_LON` to the capture geography for soft CPR (script has defaults; clear either to omit). Override the fixture with `SAMPLE_SC16=…` if needed.
