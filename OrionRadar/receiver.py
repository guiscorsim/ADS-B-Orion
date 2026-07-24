"""Standalone CLI ADS-B receiver (UDP). Conflicts with server.py on the same port."""

import socket
import time

from adsb_track import UDP_IP, UDP_PORT, TrackState, recv_df17


def main() -> None:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind((UDP_IP, UDP_PORT))
    print(f"ADS-B Receiver (pyModeS v3) listening on {UDP_IP}:{UDP_PORT}")

    state_cache: dict[str, TrackState] = {}
    total_received = 0
    decoded_positions = 0
    start_time = time.time()
    last_print = 0.0

    try:
        while True:
            got, _cache, position_updated = recv_df17(sock, state_cache)
            if not got:
                continue

            total_received += 1
            if position_updated:
                decoded_positions += 1

            now = time.time()
            if now - last_print < 1.0:
                continue
            last_print = now

            print("\n--- Real-Time ADS-B Telemetry (pyModeS v3) ---")
            print(
                f"Uptime: {int(now - start_time)}s | "
                f"Msgs Rcvd: {total_received} | Decoded Pos: {decoded_positions}"
            )
            for ac_icao, info in state_cache.items():
                if info["lat"] is not None and info["lon"] is not None:
                    pos_str = f"Lat: {info['lat']:.4f}, Lon: {info['lon']:.4f}"
                else:
                    pos_str = "Position Unknown"

                alt_str = f"{info['alt']} ft" if info["alt"] is not None else "Unknown"
                spd_str = (
                    f"{info['speed']} kt" if info["speed"] is not None else "Unknown"
                )
                hdg_str = (
                    f"{info['heading']}°" if info["heading"] is not None else "Unknown"
                )
                state_type = "[GROUND]" if info["is_surface"] else "[AIRBORNE]"
                print(
                    f"ICAO: {ac_icao} {state_type} | {pos_str} | "
                    f"Alt: {alt_str} | Vel: {spd_str} | Hdg: {hdg_str}"
                )

    except KeyboardInterrupt:
        print("\nReceiver stopped.")


if __name__ == "__main__":
    main()
