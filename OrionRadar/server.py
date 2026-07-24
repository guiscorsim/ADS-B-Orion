import socket
import time
import threading
import pyModeS as pms
import pyModeS.position as pos_decoder
from flask import Flask, jsonify, send_from_directory
from datetime import datetime
import os
import csv

UDP_IP = "127.0.0.1"
UDP_PORT = 20000

# Create telemetry folder
os.makedirs("telemetry", exist_ok=True)
timestamp_str = datetime.now().strftime("%Y%m%d_%H%M%S")
CSV_FILE = os.path.join("telemetry", f"telemetry_log_{timestamp_str}.csv")

if not os.path.exists(CSV_FILE):
    with open(CSV_FILE, 'w', newline='') as f:
        writer = csv.writer(f)
        writer.writerow(['timestamp', 'icao', 'type', 'lat', 'lon', 'alt', 'speed', 'heading'])

def log_to_csv(cache):
    with open(CSV_FILE, 'a', newline='') as f:
        writer = csv.writer(f)
        ctype = 'GROUND' if cache['is_surface'] else 'AIRBORNE'
        writer.writerow([
            time.time(),
            cache['icao'],
            ctype,
            cache['lat'],
            cache['lon'],
            cache.get('alt', ''),
            cache.get('speed', ''),
            cache.get('heading', '')
        ])

# Cache for decoding positions (requires even and odd messages)
state_cache = {}

# Load Aircraft Database
aircraft_db = {}
try:
    with open('aircraft_database.csv', 'r', encoding='utf-8') as f:
        reader = csv.DictReader(f)
        for row in reader:
            icao_hex = row['icao24'].upper()
            aircraft_db[icao_hex] = {
                'registration': row['registration'],
                'model': f"{row['manufacturer']} {row['model']}",
                'operator': row['operator'],
                'category': row['category'],
                'serial_number': row['serial_number'],
                'max_pax': row['max_pax'],
                'status': row['status'],
                'validity': row['validity'],
                'year': row['year'],
                'engine': row['engine'],
                'anac_b64': row['anac_b64']
            }
except Exception as e:
    print("Could not load database:", e)

app = Flask(__name__, static_folder='static', static_url_path='')

def udp_listener():
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind((UDP_IP, UDP_PORT))
    print(f"Background UDP Receiver listening on {UDP_IP}:{UDP_PORT}")
    
    while True:
        try:
            data, addr = sock.recvfrom(1024)
            msg_hex = data.decode('utf-8').strip()
            
            if len(msg_hex) != 28:
                continue
                
            msg = pms.Message(msg_hex)
            
            if msg.df != 17:
                continue
                
            icao = msg.icao
            tc = msg.typecode
            
            if icao not in state_cache:
                state_cache[icao] = {
                    'icao': icao,
                    'even_cpr': None, 'even_time': 0,
                    'odd_cpr': None, 'odd_time': 0, 
                    'lat': None, 'lon': None, 'alt': None, 
                    'speed': None, 'heading': None,
                    'is_surface': False,
                    'callsign': 'Waiting...',
                    'adsb_version': 'Waiting...',
                    'adsb_category': 'Waiting...',
                    'last_seen': time.time()
                }
            
            cache = state_cache[icao]
            current_time = time.time()
            cache['last_seen'] = current_time
            
            decoded = msg.decode()
            
            if 'callsign' in decoded:
                cache['callsign'] = decoded['callsign'].strip()
            if 'version' in decoded:
                cache['adsb_version'] = decoded['version']
            if 'wake_vortex' in decoded:
                cache['adsb_category'] = decoded['wake_vortex']
            
            if 9 <= tc <= 18:
                # Airborne Position
                cache['is_surface'] = False
                cache['alt'] = decoded.get('altitude')
                
                cpr_format = decoded.get('cpr_format')
                cpr_lat = decoded.get('cpr_lat')
                cpr_lon = decoded.get('cpr_lon')
                
                if cpr_format == 0:
                    cache['even_cpr'] = (cpr_lat, cpr_lon)
                    cache['even_time'] = current_time
                elif cpr_format == 1:
                    cache['odd_cpr'] = (cpr_lat, cpr_lon)
                    cache['odd_time'] = current_time
                    
                if cache['even_cpr'] and cache['odd_cpr']:
                    even_is_newer = cache['even_time'] >= cache['odd_time']
                    pos = pos_decoder.airborne_position_pair(
                        cache['even_cpr'][0], cache['even_cpr'][1],
                        cache['odd_cpr'][0], cache['odd_cpr'][1],
                        even_is_newer=even_is_newer
                    )
                    if pos:
                        cache['lat'] = pos[0]
                        cache['lon'] = pos[1]
                        log_to_csv(cache)
                        
            elif 5 <= tc <= 8:
                # Surface Position
                cache['is_surface'] = True
                cache['speed'] = decoded.get('groundspeed')
                cache['heading'] = decoded.get('track')
                cache['alt'] = 0
                
                cpr_format = decoded.get('cpr_format')
                cpr_lat = decoded.get('cpr_lat')
                cpr_lon = decoded.get('cpr_lon')
                
                if cpr_format == 0:
                    cache['even_cpr'] = (cpr_lat, cpr_lon)
                    cache['even_time'] = current_time
                elif cpr_format == 1:
                    cache['odd_cpr'] = (cpr_lat, cpr_lon)
                    cache['odd_time'] = current_time
                    
                if cache['even_cpr'] and cache['odd_cpr']:
                    even_is_newer = cache['even_time'] >= cache['odd_time']
                    lat_ref, lon_ref = -23.627, -46.656
                    pos = pos_decoder.surface_position_pair(
                        cache['even_cpr'][0], cache['even_cpr'][1],
                        cache['odd_cpr'][0], cache['odd_cpr'][1],
                        lat_ref=lat_ref, lon_ref=lon_ref,
                        even_is_newer=even_is_newer
                    )
                    if pos:
                        cache['lat'] = pos[0]
                        cache['lon'] = pos[1]
                        log_to_csv(cache)

            elif tc == 19:
                # Airborne Velocity
                cache['speed'] = decoded.get('groundspeed')
                cache['heading'] = decoded.get('track')

        except Exception as e:
            # Silently drop failed decodes in the background
            pass

@app.route('/')
def index():
    return send_from_directory('static', 'index.html')

@app.route('/api/aircraft')
def get_aircraft():
    import base64
    current_time = time.time()
    active_aircraft = []
    
    keys_to_remove = []
    for icao, data in state_cache.items():
        if current_time - data['last_seen'] > 15:
            keys_to_remove.append(icao)
        else:
            if data['lat'] is not None and data['lon'] is not None:
                metadata = aircraft_db.get(icao, {
                    'registration': 'UNKNOWN',
                    'model': 'UNKNOWN',
                    'operator': 'UNKNOWN',
                    'category': 'AIRBORNE',
                    'serial_number': 'UNKNOWN',
                    'max_pax': 'UNKNOWN',
                    'status': 'UNKNOWN',
                    'validity': 'UNKNOWN',
                    'year': 'UNKNOWN',
                    'engine': 'UNKNOWN',
                    'anac_b64': ''
                })
                
                anac_data = {}
                if metadata.get('anac_b64', ''):
                    try:
                        decoded = base64.b64decode(metadata['anac_b64']).decode('utf-8')
                        import json
                        anac_data = json.loads(decoded)
                    except:
                        pass
                
                active_aircraft.append({
                    'icao': data['icao'],
                    'lat': data['lat'],
                    'lon': data['lon'],
                    'alt': data['alt'],
                    'speed': data['speed'],
                    'heading': data['heading'],
                    'is_surface': data['is_surface'],
                    'last_seen': data['last_seen'],
                    'telemetry_callsign': data.get('callsign', 'Waiting...'),
                    'telemetry_version': data.get('adsb_version', 'Waiting...'),
                    'telemetry_category': data.get('adsb_category', 'Waiting...'),
                    'registration': metadata['registration'],
                    'model': metadata['model'],
                    'operator': metadata['operator'],
                    'category': metadata['category'],
                    'serial_number': metadata['serial_number'],
                    'max_pax': metadata['max_pax'],
                    'status': metadata['status'],
                    'validity': metadata['validity'],
                    'year': metadata['year'],
                    'engine': metadata['engine'],
                    'anac': anac_data
                })
                
    for k in keys_to_remove:
        del state_cache[k]
        
    return jsonify(active_aircraft)

if __name__ == "__main__":
    t = threading.Thread(target=udp_listener, daemon=True)
    t.start()
    
    print("Starting Web Server at http://127.0.0.1:5000")
    app.run(host='0.0.0.0', port=5000, debug=False, use_reloader=False)
