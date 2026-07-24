"""Shared DF17 track state and CPR position decode."""

from __future__ import annotations

import socket
import threading
import time
from typing import TypedDict

import pyModeS as pms
import pyModeS.position as pos_decoder

UDP_IP = "127.0.0.1"
UDP_PORT = 20000
SURFACE_REF = (-23.627, -46.656)

CprPair = tuple[int, int]


class TrackState(TypedDict):
    icao: str
    even_cpr: CprPair | None
    even_time: float
    odd_cpr: CprPair | None
    odd_time: float
    lat: float | None
    lon: float | None
    alt: float | None
    speed: float | None
    heading: float | None
    is_surface: bool
    callsign: str
    adsb_version: str | int
    adsb_category: str | int
    last_seen: float


def new_track(icao: str, now: float | None = None) -> TrackState:
    return {
        "icao": icao,
        "even_cpr": None,
        "even_time": 0.0,
        "odd_cpr": None,
        "odd_time": 0.0,
        "lat": None,
        "lon": None,
        "alt": None,
        "speed": None,
        "heading": None,
        "is_surface": False,
        "callsign": "Waiting...",
        "adsb_version": "Waiting...",
        "adsb_category": "Waiting...",
        "last_seen": time.time() if now is None else now,
    }


def store_cpr(
    cache: TrackState,
    cpr_format: object,
    lat: object,
    lon: object,
    now: float,
) -> None:
    if not (
        isinstance(lat, (int, float))
        and isinstance(lon, (int, float))
        and not isinstance(lat, bool)
        and not isinstance(lon, bool)
    ):
        return
    pair = (int(lat), int(lon))
    if cpr_format == 0:
        cache["even_cpr"] = pair
        cache["even_time"] = now
    elif cpr_format == 1:
        cache["odd_cpr"] = pair
        cache["odd_time"] = now


def resolve_position(cache: TrackState) -> tuple[float, float] | None:
    even = cache["even_cpr"]
    odd = cache["odd_cpr"]
    if even is None or odd is None:
        return None
    even_is_newer = cache["even_time"] >= cache["odd_time"]
    if cache["is_surface"]:
        return pos_decoder.surface_position_pair(
            even[0],
            even[1],
            odd[0],
            odd[1],
            lat_ref=SURFACE_REF[0],
            lon_ref=SURFACE_REF[1],
            even_is_newer=even_is_newer,
        )
    return pos_decoder.airborne_position_pair(
        even[0],
        even[1],
        odd[0],
        odd[1],
        even_is_newer=even_is_newer,
    )


def _as_float(value: object) -> float | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        return float(value)
    return None


def _apply_velocity(cache: TrackState, decoded: dict) -> None:
    speed = _as_float(decoded.get("groundspeed"))
    heading = _as_float(decoded.get("track"))
    if speed is not None:
        cache["speed"] = speed
    if heading is not None:
        cache["heading"] = heading


def _apply_cpr_position(cache: TrackState, decoded: dict, now: float) -> bool:
    store_cpr(
        cache,
        decoded.get("cpr_format"),
        decoded.get("cpr_lat"),
        decoded.get("cpr_lon"),
        now,
    )
    pos = resolve_position(cache)
    if pos is None:
        return False
    cache["lat"], cache["lon"] = pos
    return True


def _clear_cpr(cache: TrackState) -> None:
    cache["even_cpr"] = None
    cache["even_time"] = 0.0
    cache["odd_cpr"] = None
    cache["odd_time"] = 0.0


def _set_surface(cache: TrackState, is_surface: bool) -> None:
    if cache["is_surface"] != is_surface:
        _clear_cpr(cache)
    cache["is_surface"] = is_surface


def apply_df17(
    msg_hex: str,
    tracks: dict[str, TrackState],
    now: float | None = None,
) -> tuple[TrackState | None, bool]:
    """Apply one DF17 hex message.

    Returns ``(track, position_updated)``. ``track`` is None when the
    message is ignored (wrong length / DF / missing ICAO or type code).
    """
    if len(msg_hex) != 28:
        return None, False

    msg = pms.Message(msg_hex)
    if msg.df != 17:
        return None, False

    icao = msg.icao
    tc = msg.typecode
    if icao is None or tc is None:
        return None, False

    current = time.time() if now is None else now
    if icao not in tracks:
        tracks[icao] = new_track(icao, current)
    cache = tracks[icao]
    cache["last_seen"] = current

    decoded = msg.decode()
    if "callsign" in decoded:
        cache["callsign"] = str(decoded["callsign"]).strip()
    if "version" in decoded:
        version = decoded["version"]
        if isinstance(version, (str, int)):
            cache["adsb_version"] = version
    if "wake_vortex" in decoded:
        category = decoded["wake_vortex"]
        if isinstance(category, (str, int)):
            cache["adsb_category"] = category

    position_updated = False

    if 9 <= tc <= 18:
        _set_surface(cache, False)
        alt = _as_float(decoded.get("altitude"))
        if alt is not None:
            cache["alt"] = alt
        position_updated = _apply_cpr_position(cache, decoded, current)

    elif 5 <= tc <= 8:
        _set_surface(cache, True)
        cache["alt"] = 0.0
        _apply_velocity(cache, decoded)
        position_updated = _apply_cpr_position(cache, decoded, current)

    elif tc == 19:
        _apply_velocity(cache, decoded)

    return cache, position_updated


def recv_df17(
    sock: socket.socket,
    tracks: dict[str, TrackState],
    lock: threading.Lock | None = None,
) -> tuple[bool, TrackState | None, bool]:
    """Receive one UDP datagram and apply DF17.

    Returns ``(got_datagram, track, position_updated)``.
    ``got_datagram`` is False on socket/UTF-8 failures (nothing useful received).
    """
    try:
        data, _addr = sock.recvfrom(1024)
    except OSError:
        return False, None, False

    try:
        msg_hex = data.decode("utf-8").strip()
    except UnicodeDecodeError:
        return False, None, False

    def _apply() -> tuple[bool, TrackState | None, bool]:
        try:
            track, updated = apply_df17(msg_hex, tracks)
            return True, track, updated
        except Exception:  # noqa: BLE001 — keep UDP loop alive on decode quirks
            return True, None, False

    if lock is None:
        return _apply()
    with lock:
        return _apply()
