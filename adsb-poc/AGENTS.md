# AGENTS.md

## Project

POC for [CubeDesign ADS-B](adsb_mission.md): hand-rolled Rust IQ demod + DF17/18 decode (`adsb_decoder`), benched against [readsb](https://github.com/wiedehopf/readsb). Scope is **mission fields only** — ICAO, position, altitude, velocity — not full Mode S parity with readsb.

```
crates/adsb_decoder/   # IQ → DF17/18
bench/                 # fixture prep + mission-metric compare
flake.nix              # Linux: readsb, rustc/cargo, ffmpeg, hyperfine
adsb_mission.md        # HLR-ADS requirements (source of truth for goals)
```

## Environment

- Prefer Nix: `direnv allow` or `nix develop` (needs flakes).
- Dev shell provides: `readsb`, `rustc`, `cargo`, `rustfmt`, `clippy`, `ffmpeg`, `hyperfine`.
- Do not commit IQ fixtures or bench outputs (`bench/sample.*`, `bench/out/` — see `.gitignore`).

## Commands

```bash
cargo build --release
cargo test -p adsb_decoder
cargo fmt && cargo clippy -p adsb_decoder -- -D warnings

cargo run -p adsb_decoder --release -- \
  --ifile bench/sample.sc16 --iformat sc16 --stats --jsonl out.jsonl

./bench/prepare.sh          # sample.wav → sample.sc16 @ 2.4 MHz stereo s16le
./bench/run.sh              # full fixture: readsb vs adsb_decoder → bench/out/
```

Bench always uses the full prepared fixture (`bench/sample.sc16` by default). Overrides: `SAMPLE_WAV`, `SAMPLE_SC16`, `BENCH_OUT`, `REF_LAT` / `REF_LON` (soft CPR; `run.sh` defaults to UK sample scene; clear either to omit refs).
## Architecture (`adsb_decoder`)

Pipeline: SC16 IQ → magnitude (`iq`) → preamble/bit demod (`demod`) → Mode S accept/CRC (`mode_s`) → ADS-B ME / CPR (`adsb`, `cpr`) → JSONL / tracks / stats (`sink`). Chunked producer∥consumer in `pipeline`.

| Module     | Role |
|------------|------|
| `iq`       | SC16 magnitude; sample rate 2.4 Msps |
| `demod`    | Preamble detect, bit slicing → `RawFrame` |
| `mode_s`   | DF, ICAO, CRC-24, DF11 repair / learning |
| `adsb`     | TC decode: alt, vel, CPR, callsign |
| `cpr`      | Airborne/surface CPR + ≤180 NM sanity |
| `pipeline` | Chunked mag→demod (file + test helpers) |
| `sink`     | JSONL, `--stats`, optional `--tracks` |
| `frame`    | Decoded frame types for emit |

Defaults that matter:

- Input: interleaved SC16 only (`--iformat sc16`).
- Emit DF17/18 only unless `--all-df` (other DFs still feed ICAO learning).
- Timestamps from sample index.
- Soft ≤20 s airborne CPR needs `--ref-lat`/`--ref-lon` (both or neither) or a prior fix, plus ≤180 NM check; never cold-start soft CPR.
- `--tracks`: `.jsonl` = one aircraft per line; otherwise a JSON array.

## Agent rules

1. **Mission first.** Prefer changes that improve HLR-ADS-02 fields (ICAO / lat-lon / altitude / velocity) or fair comparison with readsb in `bench/run.sh`. Do not expand into full Mode S / dump1090 feature parity unless asked.
2. **Keep the decoder hand-rolled.** No new heavy crates for demod/CRC/CPR; stick to workspace deps (`clap`, `memmap2`, `serde`, `serde_json`) unless there is a clear need and the user agrees.
3. **Match existing style.** Small focused modules, module-level `//!` docs, Rust 2021, clap derive CLI. Prefer extending existing types over new abstraction layers (e.g. sink is concrete `BenchSink`, not a trait soup).
4. **Verify with tests + bench when touching decode/CPR.** Unit tests live next to modules (`#[cfg(test)]`). After CPR / accept / ME changes, run `cargo test -p adsb_decoder`; if a fixture exists, run `./bench/run.sh` and check mission metrics in `bench/out/summary.*`.
5. **Do not invent fixtures.** Never generate fake IQ into git. Use `./bench/prepare.sh` with the real `bench/sample.wav`; leave WAV/SC16 local.
6. **Docs stay lean.** Update `README.md` only when CLI, fixture, or bench contract changes. Mission requirements live in `adsb_mission.md` — do not contradict them.
7. **Nix is the toolchain source of truth.** Add tools to `flake.nix` if new CLI deps are required for build/bench.

## Out of scope (unless explicitly requested)

- Full Mode S / DF coverage beyond what helps DF17/18 learning
- Onboard TM/UART sinks, CubeSat hardware, or ground-station UI
- Committing large binary IQ captures
- Rewriting the bench Python mission decoder unless metrics are wrong
