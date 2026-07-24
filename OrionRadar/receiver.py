import socket
import time
import pyModeS as pms
import pyModeS.position as pos_decoder

UDP_IP = "127.0.0.1"
UDP_PORT = 20000

def main():
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind((UDP_IP, UDP_PORT))
    print(f"ADS-B Receiver (pyModeS v3) listening on {UDP_IP}:{UDP_PORT}")
    
    # Cache for decoding positions (requires even and odd messages)
    state_cache = {}
    
    total_received = 0
    decoded_positions = 0
    
    start_time = time.time()

    try:
        while True:
            data, addr = sock.recvfrom(1024)
            msg_hex = data.decode('utf-8').strip()
            total_received += 1
            
            if len(msg_hex) != 28:
                continue
                
            try:
                msg = pms.Message(msg_hex)
                
                if msg.df != 17:
                    continue
                    
                icao = msg.icao
                tc = msg.typecode
                
                if icao not in state_cache:
                    state_cache[icao] = {
                        'even_cpr': None, 'even_time': 0,
                        'odd_cpr': None, 'odd_time': 0, 
                        'last_pos': None, 'alt': None, 
                        'speed': None, 'heading': None,
                        'is_surface': False
                    }
                
                cache = state_cache[icao]
                current_time = time.time()
                
                decoded = msg.decode()
                
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
                            cache['last_pos'] = pos
                            decoded_positions += 1
                            
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
                            cache['last_pos'] = pos
                            decoded_positions += 1

                elif tc == 19:
                    # Airborne Velocity
                    cache['speed'] = decoded.get('groundspeed')
                    cache['heading'] = decoded.get('track')

                # Print Telemetry
                if total_received % 10 == 0:
                    print("\n--- Real-Time ADS-B Telemetry (pyModeS v3) ---")
                    print(f"Uptime: {int(current_time - start_time)}s | Msgs Rcvd: {total_received} | Decoded Pos: {decoded_positions}")
                    for ac_icao, info in state_cache.items():
                        if info['last_pos']:
                            pos_str = f"Lat: {info['last_pos'][0]:.4f}, Lon: {info['last_pos'][1]:.4f}"
                        else:
                            pos_str = "Position Unknown"
                            
                        alt_str = f"{info['alt']} ft" if info['alt'] is not None else "Unknown"
                        spd_str = f"{info['speed']} kt" if info['speed'] is not None else "Unknown"
                        hdg_str = f"{info['heading']}°" if info['heading'] is not None else "Unknown"
                        
                        state_type = "[GROUND]" if info['is_surface'] else "[AIRBORNE]"
                        print(f"ICAO: {ac_icao} {state_type} | {pos_str} | Alt: {alt_str} | Vel: {spd_str} | Hdg: {hdg_str}")
                        
            except Exception as e:
                # print(f"Decode error: {e}") # Uncomment to debug
                pass

    except KeyboardInterrupt:
        print("\nReceiver stopped.")

if __name__ == "__main__":
    main()
