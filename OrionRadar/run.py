import os
import socket
import subprocess
import sys
import time
from collections.abc import Callable

from adsb_track import UDP_IP, UDP_PORT

ANAC_SOURCE = "dados_aeronaves.csv"
AIRCRAFT_DB = "aircraft_database.csv"
RADAR_HOST = "127.0.0.1"
RADAR_PORT = 5000
RADAR_URL = f"http://{RADAR_HOST}:{RADAR_PORT}"
UDP_ENDPOINT = f"{UDP_IP}:{UDP_PORT}"

BOX_INNER = 38
SERVER_READY_TIMEOUT_S = 15.0
SIMULATOR_SETTLE_S = 0.5
SIMULATOR_READY_TIMEOUT_S = 5.0


def _print_banner() -> None:
    title = "OrionRadar"
    prefix = f"─ {title} "
    pad = max(BOX_INNER - len(prefix), 1)
    print("┌" + prefix + ("─" * pad) + "┐")
    for label, value in (("map", RADAR_URL), ("udp", UDP_ENDPOINT)):
        content = f"  {label:<5}{value}"
        if len(content) > BOX_INNER:
            content = content[: BOX_INNER - 1] + "…"
        print("│" + content.ljust(BOX_INNER) + "│")
    print("└" + ("─" * BOX_INNER) + "┘")
    print()


def _step(label: str, status: str) -> None:
    print(f"  · {label:<26} {status}")


def _stop(process: subprocess.Popen) -> None:
    if process.poll() is None:
        process.terminate()
        process.wait()


def _stop_all(*processes: subprocess.Popen) -> None:
    for process in processes:
        _stop(process)


def _abort(label: str, detail: str, *processes: subprocess.Popen) -> None:
    _step(label, "fail")
    print(f"    {detail}")
    _stop_all(*processes)
    sys.exit(1)


def _wait_until(
    predicate: Callable[[], bool],
    *,
    timeout_s: float,
    process: subprocess.Popen | None = None,
    poll_s: float = 0.1,
) -> str | None:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if process is not None and process.poll() is not None:
            return f"process exited {process.returncode}"
        if predicate():
            if process is not None and process.poll() is not None:
                return f"process exited {process.returncode}"
            return None
        time.sleep(poll_s)
    if process is not None and process.poll() is not None:
        return f"process exited {process.returncode}"
    return f"timed out after {timeout_s:.0f}s"


def _tcp_open(host: str, port: int) -> bool:
    try:
        with socket.create_connection((host, port), timeout=0.2):
            return True
    except OSError:
        return False


def _port_busy(host: str, port: int, *, sock_type: int) -> bool:
    sock = socket.socket(socket.AF_INET, sock_type)
    if sock_type == socket.SOCK_STREAM:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        sock.bind((host, port))
        return False
    except OSError:
        return True
    finally:
        sock.close()


def main() -> None:
    _print_banner()

    if not os.path.isfile(ANAC_SOURCE):
        _abort(
            "sample ANAC fleet",
            f"missing {ANAC_SOURCE} (tracked in the repo)",
        )

    try:
        subprocess.run(
            [sys.executable, "prepare_db.py"],
            check=True,
            stdout=subprocess.DEVNULL,
        )
    except subprocess.CalledProcessError as e:
        _abort(
            "sample ANAC fleet",
            f"prepare_db.py exited {e.returncode} — check {ANAC_SOURCE}",
        )

    if not os.path.isfile(AIRCRAFT_DB) or os.path.getsize(AIRCRAFT_DB) == 0:
        _abort("sample ANAC fleet", f"{AIRCRAFT_DB} was not created")
    _step("sample ANAC fleet", "ok")

    busy: list[str] = []
    if _port_busy(RADAR_HOST, RADAR_PORT, sock_type=socket.SOCK_STREAM):
        busy.append(f"tcp {RADAR_HOST}:{RADAR_PORT}")
    if _port_busy(UDP_IP, UDP_PORT, sock_type=socket.SOCK_DGRAM):
        busy.append(f"udp {UDP_IP}:{UDP_PORT}")
    if busy:
        _abort(
            "ports available",
            f"{', '.join(busy)} already in use — stop the other OrionRadar instance",
        )

    child_env = {**os.environ, "ORION_MANAGED": "1"}
    server_process = subprocess.Popen(
        [sys.executable, "server.py"],
        env=child_env,
        start_new_session=True,
    )
    server_err = _wait_until(
        lambda: _tcp_open(RADAR_HOST, RADAR_PORT),
        timeout_s=SERVER_READY_TIMEOUT_S,
        process=server_process,
    )
    if server_err is not None:
        _abort("web + UDP receiver", server_err, server_process)
    _step("web + UDP receiver", "ok")

    simulator_process = subprocess.Popen(
        [sys.executable, "simulator.py"],
        env=child_env,
        start_new_session=True,
    )
    started = time.monotonic()
    sim_err = _wait_until(
        lambda: time.monotonic() - started >= SIMULATOR_SETTLE_S,
        timeout_s=SIMULATOR_READY_TIMEOUT_S,
        process=simulator_process,
    )
    if sim_err is not None:
        _abort("ADS-B simulator", sim_err, simulator_process, server_process)
    _step("ADS-B simulator", "ok")

    print()
    print("  listening — Ctrl+C to stop")
    print()

    try:
        server_process.wait()
    except KeyboardInterrupt:
        print()
        _step("shutdown", "ok")
        _stop_all(server_process, simulator_process)


if __name__ == "__main__":
    main()
