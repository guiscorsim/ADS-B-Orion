import base64
import binascii
import csv
import json
import logging
import os
import socket
import threading
import time
from contextlib import suppress
from datetime import UTC, datetime
from typing import Any

from flask import Flask, jsonify, send_from_directory

from adsb_track import UDP_IP, UDP_PORT, TrackState, recv_df17

os.makedirs("telemetry", exist_ok=True)
CSV_FILE = os.path.join(
    "telemetry",
    f"telemetry_log_{datetime.now(UTC).strftime('%Y%m%d_%H%M%S')}.csv",
)
_csv_fh = open(CSV_FILE, "w", newline="")  # noqa: SIM115 — kept open for the process lifetime
_csv_writer = csv.writer(_csv_fh)
_csv_writer.writerow(
    ["timestamp", "icao", "type", "lat", "lon", "alt", "speed", "heading"]
)
_csv_fh.flush()
_csv_rows_since_flush = 0
_CSV_FLUSH_EVERY = 20

DEFAULT_META: dict[str, Any] = {
    "registration": "UNKNOWN",
    "model": "UNKNOWN",
    "operator": "UNKNOWN",
    "category": "AIRBORNE",
    "serial_number": "UNKNOWN",
    "max_pax": "UNKNOWN",
    "status": "UNKNOWN",
    "validity": "UNKNOWN",
    "year": "UNKNOWN",
    "engine": "UNKNOWN",
    "anac": {},
}


def _decode_anac(anac_b64: str) -> dict[str, Any]:
    if not anac_b64:
        return {}
    with suppress(
        ValueError,
        UnicodeDecodeError,
        json.JSONDecodeError,
        TypeError,
        binascii.Error,
    ):
        return json.loads(base64.b64decode(anac_b64).decode("utf-8"))
    return {}


def log_to_csv(cache: TrackState) -> None:
    global _csv_rows_since_flush
    ctype = "GROUND" if cache["is_surface"] else "AIRBORNE"
    _csv_writer.writerow(
        [
            time.time(),
            cache["icao"],
            ctype,
            cache["lat"],
            cache["lon"],
            cache["alt"] if cache["alt"] is not None else "",
            cache["speed"] if cache["speed"] is not None else "",
            cache["heading"] if cache["heading"] is not None else "",
        ]
    )
    _csv_rows_since_flush += 1
    if _csv_rows_since_flush >= _CSV_FLUSH_EVERY:
        _csv_fh.flush()
        _csv_rows_since_flush = 0


state_cache: dict[str, TrackState] = {}
state_lock = threading.Lock()

aircraft_db: dict[str, dict[str, Any]] = {}
try:
    with open("aircraft_database.csv", encoding="utf-8") as f:
        reader = csv.DictReader(f)
        for row in reader:
            icao_hex = row["icao24"].upper()
            aircraft_db[icao_hex] = {
                "registration": row["registration"],
                "model": f"{row['manufacturer']} {row['model']}",
                "operator": row["operator"],
                "category": row["category"],
                "serial_number": row["serial_number"],
                "max_pax": row["max_pax"],
                "status": row["status"],
                "validity": row["validity"],
                "year": row["year"],
                "engine": row["engine"],
                "anac": _decode_anac(row["anac_b64"]),
            }
except (OSError, csv.Error, KeyError, UnicodeDecodeError) as e:
    print("Could not load database:", e)

app = Flask(__name__, static_folder="static", static_url_path="")


def udp_listener() -> None:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind((UDP_IP, UDP_PORT))
    if os.environ.get("ORION_MANAGED") != "1":
        print(f"UDP receiver on {UDP_IP}:{UDP_PORT}")

    while True:
        got, cache, position_updated = recv_df17(sock, state_cache, lock=state_lock)
        if got and position_updated and cache is not None:
            log_to_csv(cache)


@app.route("/")
def index():
    return send_from_directory("static", "index.html")


@app.route("/api/aircraft")
def get_aircraft():
    current_time = time.time()
    active_aircraft = []
    keys_to_remove: list[str] = []

    with state_lock:
        items = list(state_cache.items())
        for icao, data in items:
            if current_time - data["last_seen"] > 15:
                keys_to_remove.append(icao)
            elif data["lat"] is not None and data["lon"] is not None:
                metadata = aircraft_db.get(icao, DEFAULT_META)
                active_aircraft.append(
                    {
                        "icao": data["icao"],
                        "lat": data["lat"],
                        "lon": data["lon"],
                        "alt": data["alt"],
                        "speed": data["speed"],
                        "heading": data["heading"],
                        "is_surface": data["is_surface"],
                        "last_seen": data["last_seen"],
                        "telemetry_callsign": data["callsign"],
                        "telemetry_version": data["adsb_version"],
                        "telemetry_category": data["adsb_category"],
                        "registration": metadata["registration"],
                        "model": metadata["model"],
                        "operator": metadata["operator"],
                        "category": metadata["category"],
                        "serial_number": metadata["serial_number"],
                        "max_pax": metadata["max_pax"],
                        "status": metadata["status"],
                        "validity": metadata["validity"],
                        "year": metadata["year"],
                        "engine": metadata["engine"],
                        "anac": metadata["anac"],
                    }
                )
        for k in keys_to_remove:
            del state_cache[k]

    return jsonify(active_aircraft)


if __name__ == "__main__":
    t = threading.Thread(target=udp_listener, daemon=True)
    t.start()

    # Always mute werkzeug request logs for this lab UI.
    logging.getLogger("werkzeug").setLevel(logging.ERROR)
    import flask.cli

    def _no_banner(debug: bool, app_import_path: str | None) -> None:
        return None

    flask.cli.show_server_banner = _no_banner  # ty: ignore[invalid-assignment]

    if os.environ.get("ORION_MANAGED") != "1":
        print(f"web server http://127.0.0.1:5000  (UDP {UDP_IP}:{UDP_PORT})")

    app.run(host="0.0.0.0", port=5000, debug=False, use_reloader=False)
