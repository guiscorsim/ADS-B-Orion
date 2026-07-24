#!/usr/bin/env bash
# Run two ADS-B decoders on bench/sample.sc16 (no --throttle) and collect
# CubeDesign mission metrics (HLR-ADS-02): DF17/18 counts, ICAOs with
# position / altitude / velocity, wall time. Tools: readsb, adsb_decoder.
#
# Usage:
#   ./bench/run.sh              # uses ./bench/sample.sc16
# Soft ≤20 s airborne CPR needs a sanity ref; defaults match the UK sample scene.
#   REF_LAT=51.4 REF_LON=-0.4 ./bench/run.sh
#   REF_LAT= REF_LON= ./bench/run.sh   # omit --ref-* (expect fewer pos ICAOs)
set -euo pipefail

BENCH_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$BENCH_DIR/.." && pwd)"
cd "$ROOT"

SC16="${SAMPLE_SC16:-$BENCH_DIR/sample.sc16}"
OUT_DIR="${BENCH_OUT:-bench/out}"
# Default scene ref for the UK sample fixture (London FIR traffic); empty to disable.
REF_LAT="${REF_LAT-51.4}"
REF_LON="${REF_LON--0.4}"
mkdir -p "$OUT_DIR"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: $1 not on PATH (enter nix develop / direnv allow)" >&2
    exit 1
  fi
}

need readsb
need cargo
need python3

if [[ ! -f "$SC16" ]]; then
  echo "error: missing $SC16 — run ./bench/prepare.sh first (or set SAMPLE_SC16)" >&2
  exit 1
fi

# Absolute path required by readsb file reader.
SC16_ABS="$(cd "$(dirname "$SC16")" && pwd)/$(basename "$SC16")"
SUMMARY_JSON="$OUT_DIR/summary.json"
SUMMARY_TXT="$OUT_DIR/summary.txt"
ICAO_DIR="$OUT_DIR/icaos"
mkdir -p "$ICAO_DIR"

echo "fixture: $SC16_ABS ($(du -h "$SC16_ABS" | awk '{print $1}'))"
echo "results: $OUT_DIR/"
echo "focus: DF17/18 mission fields (ICAO / position / altitude / velocity)"
echo

# Wall-clock helper: runs command, writes seconds to $1, forwards exit status.
# Usage: run_timed <seconds_file> -- cmd...
run_timed() {
  local sec_file="$1"
  shift
  [[ "${1:-}" == "--" ]] && shift
  local start end rc
  start="$(date +%s.%N)"
  # Keep -e off inside this function: readsb exits non-zero on ifile EOF,
  # and `return <nonzero>` under `set -e` aborts the whole script.
  set +e
  "$@"
  rc=$?
  end="$(date +%s.%N)"
  python3 -c "print(f'{float('$end')-float('$start'):.3f}')" >"$sec_file"
  set -e
  return 0
}

# Shared Python: DF17/18 ME decode for fair mission metrics from raw hex / JSONL.
MISSION_PY='
import json, math, sys
from pathlib import Path
from collections import defaultdict

CPR_MAX = 131072.0
NZ = 15.0

def crc24(msg: bytes) -> int:
    crc = 0
    for byte in msg:
        crc ^= byte << 16
        for _ in range(8):
            crc <<= 1
            if crc & 0x1000000:
                crc ^= 0xFFF409
    return crc & 0xFFFFFF

def nl(lat):
    lat = abs(lat)
    if lat >= 87.0:
        return 1
    if lat < 1e-6:
        return 59
    a = 1.0 - (1.0 - math.cos(math.pi / (2.0 * NZ))) / (math.cos(math.radians(lat)) ** 2)
    if a <= 0.0:
        return 1
    return int(math.floor(2.0 * math.pi / math.acos(a)))

def cpr_mod(a, n):
    r = a % n
    if r < 0:
        r += n
    return r

def decode_airborne(ye, xe, yo, xo, even_newer):
    dlat0 = 360.0 / (4.0 * NZ)
    dlat1 = 360.0 / (4.0 * NZ - 1.0)
    j = math.floor((59.0 * ye - 60.0 * yo) / CPR_MAX + 0.5)
    lat0 = dlat0 * (cpr_mod(j, 60.0) + ye / CPR_MAX)
    lat1 = dlat1 * (cpr_mod(j, 59.0) + yo / CPR_MAX)
    if lat0 >= 270: lat0 -= 360
    if lat1 >= 270: lat1 -= 360
    if nl(lat0) != nl(lat1):
        return None
    lat = lat0 if even_newer else lat1
    if not (-90.0 <= lat <= 90.0):
        return None
    nl_val = float(nl(lat))
    m = math.floor((xe * (nl_val - 1.0) - xo * nl_val) / CPR_MAX + 0.5)
    if even_newer:
        ni = max(nl_val, 1.0)
        lon_cpr = xe
    else:
        ni = max(nl_val - 1.0, 1.0)
        lon_cpr = xo
    dlon = 360.0 / ni
    lon = dlon * (cpr_mod(m, ni) + lon_cpr / CPR_MAX)
    if lon >= 180: lon -= 360
    if not (-180.0 <= lon <= 180.0):
        return None
    return lat, lon

def parse_cpr(msg):
    f_odd = (msg[6] & 0x04) != 0
    lat = (((msg[6] & 0x03) << 15) | (msg[7] << 7) | (msg[8] >> 1)) & 0x1FFFF
    lon = (((msg[8] & 0x01) << 16) | (msg[9] << 8) | msg[10]) & 0x1FFFF
    return f_odd, lat, lon

def decode_ac12(msg):
    alt_code = (((msg[5] << 4) | (msg[6] >> 4)) & 0x0FFF)
    if alt_code == 0:
        return None
    if (alt_code & 0x10) == 0:
        return None
    n = ((alt_code & 0x0FE0) >> 1) | (alt_code & 0x0F)
    return n * 25 - 1000

def decode_velocity(msg):
    subtype = msg[4] & 0x07
    if subtype in (1, 2):
        ew_dir = (msg[5] >> 2) & 1
        ew = ((msg[5] & 0x03) << 8) | msg[6]
        ns_dir = (msg[7] >> 7) & 1
        ns = ((msg[7] & 0x7F) << 3) | (msg[8] >> 5)
        if ew == 0 or ns == 0:
            return None, None
        vew = float(ew - 1)
        vns = float(ns - 1)
        if subtype == 2:
            vew *= 4; vns *= 4
        if ew_dir: vew = -vew
        if ns_dir: vns = -vns
        gs = math.hypot(vew, vns)
        track = math.degrees(math.atan2(vew, vns))
        if track < 0: track += 360
        return gs, track
    if subtype in (3, 4):
        as_raw = ((msg[7] & 0x7F) << 3) | (msg[8] >> 5)
        if as_raw == 0:
            return None, None
        return float(as_raw - 1), None
    return None, None

class MissionAccum:
    def __init__(self):
        self.messages = 0
        self.df17_18 = 0
        self.df_hist = defaultdict(int)
        self.icaos = set()
        self.pos = set()
        self.alt = set()
        self.vel = set()
        # icao -> {even, odd, seq}  seq is per-ICAO CPR sample order
        self.cpr = {}

    def note_icao(self, icao):
        if icao:
            self.icaos.add(icao)

    def add_frame(self, icao, df, alt=None, lat=None, lon=None, gs=None, ts=None, raw=None):
        self.messages += 1
        self.df_hist[df] += 1
        self.note_icao(icao)
        if df not in (17, 18):
            return
        self.df17_18 += 1
        if alt is not None:
            self.alt.add(icao)
        if gs is not None:
            self.vel.add(icao)
        if lat is not None and lon is not None:
            self.pos.add(icao)
            return
        if raw is None or len(raw) < 14:
            return
        tc = raw[4] >> 3
        if tc in range(9, 19) or tc in range(20, 23):
            a = decode_ac12(raw)
            if a is not None:
                self.alt.add(icao)
            odd, lat_cpr, lon_cpr = parse_cpr(raw)
            st = self.cpr.setdefault(icao, {"even": None, "odd": None, "seq": 0})
            st["seq"] += 1
            # Prefer real timestamps; else per-ICAO sequence (not global msg index).
            t = float(ts) if ts is not None else float(st["seq"])
            sample = (lat_cpr, lon_cpr, t)
            if odd:
                st["odd"] = sample
            else:
                st["even"] = sample
            e, o = st["even"], st["odd"]
            # 10s when ts is real; with per-ICAO seq allow up to 20 samples apart.
            limit = 10.0 if ts is not None else 20.0
            if e and o and abs(e[2] - o[2]) <= limit:
                even_newer = e[2] >= o[2]
                pos = decode_airborne(e[0], e[1], o[0], o[1], even_newer)
                if pos:
                    self.pos.add(icao)
        elif tc == 19:
            gs2, _ = decode_velocity(raw)
            if gs2 is not None:
                self.vel.add(icao)
        elif tc in range(5, 9):
            # Surface movement may carry speed; position needs a reference — skip.
            pass

    def summary(self, icao_source):
        return {
            "messages": self.messages,
            "crc_ok": self.messages,
            "df17_18": self.df17_18,
            "unique_icao": len(self.icaos),
            "icao_with_position": len(self.pos),
            "icao_with_altitude": len(self.alt),
            "icao_with_velocity": len(self.vel),
            "icao_source": icao_source,
            "df_hist": {str(k): v for k, v in sorted(self.df_hist.items())},
            "icaos": sorted(self.icaos),
            "icaos_pos": sorted(self.pos),
            "icaos_alt": sorted(self.alt),
            "icaos_vel": sorted(self.vel),
        }
'

parse_raw_mission() {
  local raw_file="$1"
  local out_json="$2"
  local icao_file="$3"
  python3 - "$raw_file" "$out_json" "$icao_file" <<PY
$MISSION_PY
raw_file, out_json, icao_file = sys.argv[1:4]
acc = MissionAccum()
for line in Path(raw_file).open(errors="replace"):
    line = line.strip()
    if not line:
        continue
    hx = line
    if hx.startswith("*"): hx = hx[1:]
    if hx.endswith(";"): hx = hx[:-1]
    hx = "".join(c for c in hx if c in "0123456789abcdefABCDEF")
    if len(hx) < 14:
        continue
    try:
        raw = bytes.fromhex(hx)
    except ValueError:
        continue
    df = raw[0] >> 3
    if df not in (17, 18):
        acc.df_hist[df] += 1
        continue
    if crc24(raw) != 0:
        continue
    icao = f"{raw[1]:02x}{raw[2]:02x}{raw[3]:02x}"
    # No wall-clock in --raw; CPR timeout uses per-ICAO sample order.
    acc.add_frame(icao, df, raw=raw, ts=None)
Path(icao_file).write_text("\\n".join(sorted(acc.icaos)) + ("\\n" if acc.icaos else ""))
summary = acc.summary("raw:df17-18")
# Drop bulky lists from metrics file kept for summary overlap
slim = {k: v for k, v in summary.items() if not k.startswith("icaos")}
Path(out_json).write_text(json.dumps(slim, indent=2) + "\\n")
# Keep full sets beside metrics for Jaccard
Path(out_json.with_suffix(".sets.json") if False else Path(str(out_json) + ".sets")).write_text(
    json.dumps({k: summary[k] for k in ("icaos","icaos_pos","icaos_alt","icaos_vel")}, indent=2) + "\\n"
)
print(json.dumps(slim))
PY
}

parse_jsonl_mission() {
  local jsonl="$1"
  local out_json="$2"
  local icao_file="$3"
  local icao_key="${4:-icao}"
  python3 - "$jsonl" "$out_json" "$icao_file" "$icao_key" <<PY
$MISSION_PY
path, out_json, icao_file, key = sys.argv[1:5]
acc = MissionAccum()
p = Path(path)
if p.exists():
    with p.open() as f:
        for line in f:
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                o = json.loads(line)
            except json.JSONDecodeError:
                continue
            df = o.get("df")
            try:
                df = int(df) if df is not None else None
            except (TypeError, ValueError):
                df = None
            if df is None:
                continue
            icao = o.get(key) or o.get("icao24") or o.get("icao")
            icao = str(icao).lower() if icao else ""
            if df not in (17, 18):
                acc.df_hist[df] += 1
                continue
            alt = o.get("alt")
            if alt is None:
                alt = o.get("altitude")
            lat = o.get("lat")
            if lat is None:
                lat = o.get("latitude")
            lon = o.get("lon")
            if lon is None:
                lon = o.get("longitude")
            gs = o.get("gs")
            if gs is None:
                gs = o.get("groundspeed")
            # Fall back to CPR from frame hex when lat/lon are absent
            raw = None
            fr = o.get("frame")
            if isinstance(fr, str):
                hx = "".join(c for c in fr if c in "0123456789abcdefABCDEF")
                if len(hx) >= 28:
                    try:
                        raw = bytes.fromhex(hx[:28])
                    except ValueError:
                        raw = None
            ts = o.get("ts") or o.get("timestamp")
            try:
                ts = float(ts) if ts is not None else None
            except (TypeError, ValueError):
                ts = None
            acc.add_frame(icao, df, alt=alt, lat=lat, lon=lon, gs=gs, ts=ts, raw=raw)
Path(icao_file).write_text("\\n".join(sorted(acc.icaos)) + ("\\n" if acc.icaos else ""))
summary = acc.summary(f"jsonl:{key}:df17-18")
slim = {k: v for k, v in summary.items() if not k.startswith("icaos")}
Path(out_json).write_text(json.dumps(slim, indent=2) + "\\n")
Path(str(out_json) + ".sets").write_text(
    json.dumps({k: summary[k] for k in ("icaos","icaos_pos","icaos_alt","icaos_vel")}, indent=2) + "\\n"
)
print(json.dumps(slim))
PY
}

parse_rust_stats() {
  local stats_err="$1"
  local metrics_json="$2"
  python3 - "$stats_err" "$metrics_json" <<'PY'
import json, re, sys
err_path, metrics_path = sys.argv[1], sys.argv[2]
text = open(err_path, errors="replace").read()
m = {}
for key in (
    "messages", "crc_ok", "df17_18", "unique_icao",
    "icao_with_position", "icao_with_altitude", "icao_with_velocity",
):
    match = re.search(rf"(?m)^{key}:\s*(\d+)\s*$", text)
    if match:
        m[key] = int(match.group(1))
data = json.load(open(metrics_path))
data.update({k: v for k, v in m.items()})
data["stats_stderr"] = bool(m)
json.dump(data, open(metrics_path, "w"), indent=2)
open(metrics_path, "a").write("\n")
print(json.dumps(data))
PY
}

declare -a TOOL_ORDER=()
declare -A WALL=()

echo "=== [1/2] readsb (--raw --no-fix; mission=DF17/18) ==="
set +e
run_timed "$OUT_DIR/readsb.wall" -- \
  readsb --device-type ifile --ifile "$SC16_ABS" --iformat SC16 \
    --raw --no-interactive --no-fix \
  >"$OUT_DIR/readsb.raw.txt" 2>"$OUT_DIR/readsb.stderr"
set -e
WALL[readsb]="$(cat "$OUT_DIR/readsb.wall")"
parse_raw_mission "$OUT_DIR/readsb.raw.txt" \
  "$OUT_DIR/readsb.metrics.json" "$ICAO_DIR/readsb.txt" \
  >"$OUT_DIR/readsb.parsed.json"
TOOL_ORDER+=(readsb)
echo "  wall=${WALL[readsb]}s  $(tr -d '\n' <"$OUT_DIR/readsb.metrics.json")"
echo

echo "=== [2/2] adsb_decoder (DF17/18 product path) ==="
# Build outside the timer so wall_s is decode-only (not cargo compile).
# Resolve the executable via cargo JSON so CARGO_TARGET_DIR / --target triples work.
echo "  building release binary..."
DECODER_BIN="$(
  cargo build -p adsb_decoder --release --message-format=json \
    | python3 -c '
import json, sys
exe = None
for line in sys.stdin:
    try:
        o = json.loads(line)
    except json.JSONDecodeError:
        continue
    if o.get("reason") != "compiler-artifact":
        continue
    path = o.get("executable")
    if not path:
        continue
    target = o.get("target") or {}
    if target.get("name") == "adsb_decoder" and "bin" in (target.get("kind") or []):
        exe = path
if not exe:
    sys.stderr.write("error: cargo did not report adsb_decoder executable\n")
    sys.exit(1)
print(exe)
'
)"
if [[ ! -x "$DECODER_BIN" ]]; then
  echo "error: missing $DECODER_BIN after cargo build" >&2
  exit 1
fi
echo "  binary: $DECODER_BIN"
DECODER_REF_ARGS=()
if [[ -n "${REF_LAT}" && -n "${REF_LON}" ]]; then
  DECODER_REF_ARGS=(--ref-lat "$REF_LAT" --ref-lon "$REF_LON")
  echo "  soft-CPR ref: lat=$REF_LAT lon=$REF_LON"
fi
set +e
run_timed "$OUT_DIR/adsb_decoder.wall" -- \
  "$DECODER_BIN" \
    --ifile "$SC16_ABS" --iformat sc16 \
    "${DECODER_REF_ARGS[@]}" \
    --stats --jsonl "$OUT_DIR/adsb_decoder.jsonl" \
  >"$OUT_DIR/adsb_decoder.stdout" 2>"$OUT_DIR/adsb_decoder.stderr"
set -e
WALL[adsb_decoder]="$(cat "$OUT_DIR/adsb_decoder.wall")"
parse_jsonl_mission "$OUT_DIR/adsb_decoder.jsonl" "$OUT_DIR/adsb_decoder.metrics.json" \
  "$ICAO_DIR/adsb_decoder.txt" icao >"$OUT_DIR/adsb_decoder.parsed.json"
parse_rust_stats "$OUT_DIR/adsb_decoder.stderr" "$OUT_DIR/adsb_decoder.metrics.json" \
  >"$OUT_DIR/adsb_decoder.parsed.json"
TOOL_ORDER+=(adsb_decoder)
echo "  wall=${WALL[adsb_decoder]}s  $(tr -d '\n' <"$OUT_DIR/adsb_decoder.metrics.json")"
echo

python3 - "$SUMMARY_JSON" "$SUMMARY_TXT" "$OUT_DIR" "$ICAO_DIR" "${TOOL_ORDER[@]}" <<'PY'
import json, sys
from pathlib import Path

summary_json, summary_txt, out_dir, icao_dir = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4])
tools = sys.argv[5:]
rows = []
for t in tools:
    m = json.loads((out_dir / f"{t}.metrics.json").read_text())
    wall = float((out_dir / f"{t}.wall").read_text().strip())
    sets_path = Path(str(out_dir / f"{t}.metrics.json") + ".sets")
    sets = json.loads(sets_path.read_text()) if sets_path.exists() else {}
    icao_path = icao_dir / f"{t}.txt"
    icaos = [ln.strip() for ln in icao_path.read_text().splitlines() if ln.strip()] if icao_path.exists() else []
    rows.append({
        "tool": t,
        "wall_s": wall,
        "df17_18": m.get("df17_18", m.get("messages")),
        "unique_icao": m.get("unique_icao"),
        "icao_with_position": m.get("icao_with_position"),
        "icao_with_altitude": m.get("icao_with_altitude"),
        "icao_with_velocity": m.get("icao_with_velocity"),
        "icaos": icaos,
        "icaos_pos": sets.get("icaos_pos", []),
        "metrics": m,
    })

payload = {"mission": "HLR-ADS-02 DF17/18", "tools": rows}
summary_json.write_text(json.dumps(payload, indent=2) + "\n")

lines = []
lines.append("Mission metrics (DF17/18 ADS-B — HLR-ADS-02):")
hdr = f"{'tool':<16} {'wall_s':>8} {'df17_18':>8} {'icao':>6} {'pos':>5} {'alt':>5} {'vel':>5}"
lines.append(hdr)
lines.append("-" * len(hdr))
for r in rows:
    lines.append(
        f"{r['tool']:<16} {r['wall_s']:>8.3f} {str(r['df17_18']):>8} {str(r['unique_icao']):>6} "
        f"{str(r['icao_with_position']):>5} {str(r['icao_with_altitude']):>5} {str(r['icao_with_velocity']):>5}"
    )
lines.append("")
lines.append("DF histograms (all observed; mission counts are DF17/18 only):")
for r in rows:
    hist = r["metrics"].get("df_hist")
    if hist:
        parts = ", ".join(f"DF{k}:{v}" for k, v in hist.items())
        lines.append(f"  {r['tool']}: {parts}")
lines.append("")
lines.append("ICAO set overlap (Jaccard) on DF17/18 addresses:")
sets = {r["tool"]: set(r["icaos"]) for r in rows}
for i, a in enumerate(tools):
    for b in tools[i + 1 :]:
        sa, sb = sets[a], sets[b]
        inter = len(sa & sb)
        union = len(sa | sb) or 1
        lines.append(f"  {a} ∩ {b}: {inter}  Jaccard={inter/union:.3f}  (|A|={len(sa)}, |B|={len(sb)})")
lines.append("")
lines.append("Position ICAO overlap:")
psets = {r["tool"]: set(r["icaos_pos"]) for r in rows}
for i, a in enumerate(tools):
    for b in tools[i + 1 :]:
        sa, sb = psets[a], psets[b]
        inter = len(sa & sb)
        union = len(sa | sb) or 1
        lines.append(f"  {a} ∩ {b}: {inter}  Jaccard={inter/union:.3f}  (|A|={len(sa)}, |B|={len(sb)})")
text = "\n".join(lines) + "\n"
summary_txt.write_text(text)
print(text)
print(f"wrote {summary_json}")
print(f"wrote {summary_txt}")
PY

echo "done."
