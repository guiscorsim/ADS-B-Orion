# OrionRadar - Live Tracking System & ADS-B Simulator

## 💡 Project Motivation and Intent

The main goal of this project is to provide a complete, visual, and self-sufficient laboratory for studying the **ADS-B** (Automatic Dependent Surveillance-Broadcast) protocol. Instead of relying on paid flight tracking APIs, this project simulates from scratch the physical behavior of transponders on aircraft and ground vehicles, creates air traffic, transmits the data via network packets, and renders everything on an interactive map.

OrionRadar is a complete system for simulating, processing, and visualizing aeronautical telemetry data in the ADS-B format. It allows you to simulate air and ground traffic in the São Paulo region, capture and decode ADS-B messages via UDP, and display everything in a real-time web radar interface.

## 📂 Project Structure

This folder contains exclusively the vital files for the system to work:

- `adsb_track.py`: Shared DF17 track state and CPR position decode used by `server.py` and the optional CLI `receiver.py`.
- `simulator.py`: The simulation engine. It reads aircraft from the database and generates raw hexadecimal ADS-B messages (exactly like a real transponder) for aircraft in the air (moving) and vehicles/aircraft on the ground. These messages are continuously transmitted via UDP on port `20000`.
- `server.py`: The heart of the backend. It performs two functions simultaneously:
  - Runs a Flask Web server on port `5000` that serves the visual interface.
  - Has a background thread (`Background UDP Receiver`) listening on port `20000`. It decodes the raw ADS-B messages using the `pyModeS` library, calculates coordinates (lat/lon), altitude, and speed, cross-references the information with the Brazilian Aeronautical Registry (RAB) database, saves local logs, and exposes the data through a JSON API (`/api/aircraft`).
- `receiver.py`: Optional standalone CLI decoder on the same UDP port (do not run alongside `server.py`).
- `prepare_db.py`: A data processing utility. It reads the massive original ANAC file (`dados_aeronaves.csv`) and samples random aircraft. Then, it generates enriched Base64 records (also including simulated ground fleet) to build the lean `aircraft_database.csv` database.
- `dados_aeronaves.csv`: The official raw database from the Brazilian Aeronautical Registry (RAB), containing all certified aircraft.
- `aircraft_database.csv`: The processed and mapped database by `prepare_db.py`, from which the simulator pulls its targets and from which the server pulls the complete technical data sheet (operators, owners, category, manufacturer, status).
- `static/`: Folder containing the visual resources of the Radar (Frontend):
  - `index.html`: The visual structure, divided between the statistics panel, tracking list (sidebar), map, and the dynamic advanced info modal.
  - `style.css`: All the Dark aesthetic styling, responsiveness, and layout for tables and buttons.
  - `script.js`: The frontend engine, which connects to the `/api/aircraft` route and renders the map via the `Leaflet` library. It handles the smooth animation and rotation of aircraft and vehicle icons on the screen.
- `telemetry/` (auto-generated): Folder where `server.py` keeps a historical record of all captured radar transmissions in CSV files (`telemetry_log_YYYYMMDD_HHMMSS.csv`).



## 🚀 How to Run the System

From this folder, install dependencies and start with `run.py`. Requires **Python 3.11+** (pyModeS 3).

**With [uv](https://docs.astral.sh/uv/) (recommended):**

```bash
uv sync
uv run run.py
```

**With pip:**

```bash
pip install "pandas" "pyModeS>=3" "Flask"
python run.py
```

**What the** `run.py` **script will do automatically:**

1. It will run `prepare_db.py` to pick **15 new random aircraft** from the ANAC database every time you start!
2. It will spin up `server.py` on the backend to listen for telemetry and serve the API.
3. It will spin up `simulator.py` simultaneously in the background to start transmitting aircraft positions via UDP.

Once the terminal notifies that everything is ready, just open your browser and access:

```
http://127.0.0.1:5000
```

*To shut down all components at once, simply press **CTRL+C** in the terminal.*

## 🛠 Features and Interactivity

- **Live Georeferenced Radar:** Aircraft (`cyan`) and Apron Vehicles (`orange`) will appear and move smoothly across the map. Their noses point to the exact direction of travel.
- **Auto-Cleanup:** If the simulator is turned off, icons that lose signal (telemetry) for more than 15 seconds will magically disappear from the screen (drop rate).
- **Advanced RAB Technical Data Sheets:** Clicking on any aircraft will bring up a side tab with basic data. Clicking the `Info ( i )` button will bring up a powerful Modal panel, displaying detailed data sheets mapped from ANAC, cleanly separated into tabs: `Technical Details`, `Operator(s)`, and `Owner(s)`.
- **Data Logs (Blackbox):** All processed steps are recorded in the `telemetry/` folder for future analysis and flight path auditing.



## 📡 The Magic of UDP Sockets and the ADS-B Protocol

One of the most incredible differentiators of this project is how the components talk to each other. They do not use a shared real-time database or files to exchange positions. All communication takes place via **UDP Sockets on port 20000**.

`simulator.py` acts as a radio transmitter. It assembles a 28-character hexadecimal string (Ex: `8D...`) strictly following the mathematical guidelines of the real ADS-B protocol. It simply dumps these packets onto the local network (UDP Broadcast).

On the other side, `server.py` acts as an "antenna". It listens on port 20000 completely passively and asynchronously. It does not know the data comes from a simulator. It catches the raw hex string and uses the `pyModeS` library to decrypt the bits, calculating Latitude, Longitude, Altitude, and Speed.

### 🧠 Advanced ADS-B Payload Simulation

Going way beyond GPS coordinates, **OrionRadar** is a true radio frequency laboratory based on the concepts from the book *"The 1090MHz Riddle"*:

- **Type Code 1 to 4 (Identification):** The simulator mathematically encodes the characters of the *Callsign* (Aircraft Prefix) and Wake Vortex Category using the specific aviation Base-64 bitmap.
- **Type Code 31 (Operational Status):** The simulator projects real-world scenarios by inserting **ADS-B protocol Versions (0, 1, or 2)** into masked bits (bits 41-43 of the ME field) and transmitting them intermittently (0.2 Hz).
- The backend extracts these rare packets the same way it handles tracking, dynamically filling the "ADS-B Telemetry" tabs in the frontend as the "radio signals" arrive!



### 🌐 Using Real Data (RTL-SDR and ADS-B Antennas)

You can turn OrionRadar into a **Real-Life Radar** with absolutely **ZERO modifications** to the server code!

Since our `server.py` listens to raw packets on port `20000/UDP`, it acts exactly like commercial decoders. If you have an **RTL-SDR (USB Dongle)** and an antenna tuned to **1090 MHz**, you can use software like `dump1090`.

Just configure your real receiver to *forward* the raw messages via the local network to the IP of your machine running OrionRadar, pointing to port `20000`.
Example in dump1090: `--net-ro-port 20000` or using `netcat` to mirror the raw port to UDP 20000.

As soon as the antenna picks up a real plane flying over your house, `server.py` will decode the real position, and you will see the plane appear on your interactive screen with data pulled from the ANAC fleet!

## 📦 Dependencies

Python **3.11+**, plus `pandas`, `pyModeS>=3`, and `Flask`. Prefer `uv sync` (uses `pyproject.toml` / `uv.lock` and a local `.venv`), or install with `pip` as shown above.
