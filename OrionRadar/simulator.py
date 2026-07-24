import socket
import time
import math
import random
import threading

UDP_IP = "127.0.0.1"
UDP_PORT = 20000

def crc(msg, encode=False):
    """
    Calculates the CRC (Parity) of an ADS-B message.
    msg is a binary string or integer.
    Generator polynomial: 11111111111110100000010010000000 (CRC-24Q)
    """
    if isinstance(msg, str):
        msg = int(msg, 16)
    
    generator = 0xFFFA0480
    
    # 112 bits message -> 14 bytes -> 28 hex chars
    msg_len = 112
    # Shift msg left by 24 bits for PI
    if encode:
        msg = msg << 24
        msg_len = 112
    else:
        msg_len = 112
        
    mask = 1 << (msg_len - 1)
    
    for i in range(msg_len - 24):
        if (msg & mask):
            msg ^= (generator << (msg_len - 25 - i))
        mask >>= 1
    
    return msg & 0xFFFFFF

def encode_cpr(lat, lon, t_odd, is_surface=False):
    """
    Encodes lat/lon into 17-bit CPR format.
    """
    Nz = 15
    if is_surface:
        dLat = 90.0 / (4 * Nz - int(t_odd))
    else:
        dLat = 360.0 / (4 * Nz - int(t_odd))
        
    # Calculate Lat CPR
    j = math.floor(lat / dLat) + 0.5
    lat_cpr = (lat - dLat * (j - 0.5)) / dLat * (1 << 17)
    # The actual algorithm uses a modulus math but a simpler wrapping works for small changes
    # A standard CPR encoding algorithm:
    yz = math.floor(( (lat % dLat) / dLat ) * (1 << 17) + 0.5)
    lat_cpr_int = yz & 0x1FFFF
    
    # Calculate Rlat to compute Lon CPR
    rlat = dLat * (lat_cpr_int / (1<<17) + math.floor(lat / dLat))
    
    # Number of longitude zones
    def nl(lat):
        if abs(lat) == 90: return 1
        a = math.cos(math.pi / 180.0 * lat)
        b = 1.0 - math.cos(math.pi / 180.0 * (360.0 / 60.0))
        if a**2 - b < 0: return 1
        return math.floor(2 * math.pi / math.acos(1.0 - b / (a**2)))
        
    nl_lat = nl(rlat)
    
    ni = max(1, nl_lat - int(t_odd))
    if is_surface:
        dLon = 90.0 / ni
    else:
        dLon = 360.0 / ni
    
    xz = math.floor(( (lon % dLon) / dLon ) * (1 << 17) + 0.5)
    lon_cpr_int = xz & 0x1FFFF
    
    return lat_cpr_int, lon_cpr_int

def encode_altitude(alt):
    """
    Encodes altitude into 12 bits for airborne position.
    Q bit is bit 42 (0-indexed in ME) -> 5th bit from left.
    """
    if alt < -1000: alt = -1000
    if alt > 50175: alt = 50175
    
    val = int((alt + 1000) / 25)
    
    # 12 bits: [val_left(7 bits)] [Q=1] [val_right(4 bits)]
    val_left = (val >> 4) & 0x7F
    val_right = val & 0x0F
    
    return (val_left << 5) | (1 << 4) | val_right

def build_adsb_msg(icao, me_hex):
    """
    Constructs 112-bit DF17 message
    DF(5)=17 -> 10001 = 0x11
    CA(3)=5 -> 101 = 5
    first byte = 10001101 = 0x8D
    """
    df_ca = "8D"
    icao_hex = f"{icao:06X}"
    msg_no_pi_hex = df_ca + icao_hex + me_hex
    
    msg_no_pi = int(msg_no_pi_hex, 16)
    pi = crc(msg_no_pi, encode=True)
    pi_hex = f"{pi:06X}"
    
    return msg_no_pi_hex + pi_hex

def generate_airborne_position(icao, lat, lon, alt, t_odd):
    tc = 11 # Type Code for Airborne Position (w/ baro altitude)
    ss = 0
    saf = 1 if t_odd else 0
    
    alt_enc = encode_altitude(alt)
    lat_cpr, lon_cpr = encode_cpr(lat, lon, t_odd, is_surface=False)
    
    # ME 56 bits
    # TC(5) SS(2) SAF(1) ALT(12) T(1) CPR_F(1) LAT(17) LON(17)
    me = (tc << 51) | (ss << 49) | (saf << 48) | (alt_enc << 36) | (0 << 35) | ((1 if t_odd else 0) << 34) | (lat_cpr << 17) | lon_cpr
    me_hex = f"{me:014X}"
    
    return build_adsb_msg(icao, me_hex)

def generate_surface_position(icao, lat, lon, t_odd, speed, heading):
    tc = 7 # Surface position
    
    # Movement 7 bits
    movement = 0
    if speed > 0 and speed < 0.125: movement = 1
    elif speed < 1: movement = 2
    elif speed < 2: movement = 3
    elif speed < 15: movement = int(speed) + 2 # Simplified mapping
    else: movement = 124
    
    # Status(1), Heading(7)
    status = 1
    trk = int((heading / 360.0) * 128.0) & 0x7F
    
    lat_cpr, lon_cpr = encode_cpr(lat, lon, t_odd, is_surface=True)
    
    # TC(5) MOV(7) S(1) TRK(7) T(1) F(1) LAT(17) LON(17)
    me = (tc << 51) | (movement << 44) | (status << 43) | (trk << 36) | (0 << 35) | ((1 if t_odd else 0) << 34) | (lat_cpr << 17) | lon_cpr
    me_hex = f"{me:014X}"
    
    return build_adsb_msg(icao, me_hex)

def generate_airborne_velocity(icao, speed, heading):
    tc = 19
    st = 1 # Subtype 1 = Ground speed
    ic = 0
    res = 0
    nac = 0
    
    # Speed and heading into N-S and E-W velocity
    v_n = speed * math.cos(math.radians(heading))
    v_e = speed * math.sin(math.radians(heading))
    
    # N-S sign
    d_ew = 1 if v_e < 0 else 0
    v_ew = min(1022, int(abs(v_e)) + 1)
    
    d_ns = 1 if v_n < 0 else 0
    v_ns = min(1022, int(abs(v_n)) + 1)
    
    # Vr (Vertical rate)
    vr_src = 0
    vr_sign = 0
    vr = 0
    
    # TC(5) ST(3) IC(1) RESV(1) NAC(3) DEW(1) VEW(10) DNS(1) VNS(10) VrSrc(1) VrSign(1) Vr(9) Resv(2) Diff(1) DiffAlt(7)
    me = (tc << 51) | (st << 48) | (ic << 47) | (res << 46) | (nac << 43) \
         | (d_ew << 42) | (v_ew << 32) | (d_ns << 31) | (v_ns << 21) \
         | (vr_src << 20) | (vr_sign << 19) | (vr << 10) | (0 << 8) | (0 << 7) | 0
         
    me_hex = f"{me:014X}"
    return build_adsb_msg(icao, me_hex)

def encode_callsign(icao, callsign, tc=4, ca=5):
    charset = '#ABCDEFGHIJKLMNOPQRSTUVWXYZ#####_###############0123456789######'
    callsign = (str(callsign).replace('-', '') + '        ')[:8].upper()
    val = 0
    for char in callsign:
        c_val = charset.index(char) if char in charset else 32
        val = (val << 6) | c_val
    me_hex = f"{(tc << 3 | ca):02X}" + f"{val:012X}"
    return build_adsb_msg(icao, me_hex)

def encode_status(icao, version=2, subtype=0):
    # TC=31, ST=subtype
    me_val = (31 << 51) | (subtype << 48) | (version << 13)
    me_hex = f"{me_val:014X}"
    return build_adsb_msg(icao, me_hex)

class Aircraft:
    def __init__(self, icao, lat, lon, alt, speed, heading, is_surface, callsign="UNKNOWN", adsb_version=2):
        self.icao = icao
        self.lat = lat
        self.lon = lon
        self.alt = alt
        self.speed = speed # knots
        self.heading = heading # degrees
        self.is_surface = is_surface
        self.callsign = callsign
        self.adsb_version = adsb_version
        self.t_odd = False
        self.tick = random.randint(0, 9) # Stagger transmissions
        
    def update(self, dt):
        if self.is_surface:
            # Aircraft on ground might be taxiing or parked
            if self.speed > 0:
                self.heading += random.uniform(-1, 1)
        else:
            self.heading += random.uniform(-2, 2)
            self.heading %= 360
            
        # 1 knot = 1.852 km/h. 
        # roughly 1 degree lat = 60 NM
        lat_movement = (self.speed / 3600.0 * dt) * math.cos(math.radians(self.heading)) / 60.0
        lon_movement = (self.speed / 3600.0 * dt) * math.sin(math.radians(self.heading)) / (60.0 * max(0.1, math.cos(math.radians(self.lat))))
        
        self.lat += lat_movement
        self.lon += lon_movement
        
        if self.is_surface:
            if self.lat > -23.625:
                self.lat = -23.625
                self.heading = (self.heading + 180) % 360
            elif self.lat < -23.629:
                self.lat = -23.629
                self.heading = (self.heading + 180) % 360
                
            if self.lon > -46.654:
                self.lon = -46.654
                self.heading = (self.heading + 180) % 360
            elif self.lon < -46.658:
                self.lon = -46.658
                self.heading = (self.heading + 180) % 360
                
        self.t_odd = not self.t_odd

def main():
    import csv
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    
    db_airborne = []
    db_ground = []
    try:
        with open('aircraft_database.csv', 'r', encoding='utf-8') as f:
            reader = csv.DictReader(f)
            for row in reader:
                icao = int(row['icao24'], 16)
                callsign = row['registration']
                if row['category'] == 'AIRBORNE': db_airborne.append((icao, callsign))
                else: db_ground.append((icao, callsign))
    except Exception as e:
        print("Could not load aircraft_database.csv, falling back to defaults")
        db_airborne = [(0xE00000 + i, f"FLIGHT{i}") for i in range(10)]
        db_ground = [(0xF00000 + i, f"GND{i}") for i in range(5)]

    aircraft_list = []
    
    # 10 Airborne
    for i in range(min(10, len(db_airborne))):
        icao, callsign = db_airborne[i]
        lat = -23.5 + random.uniform(-1, 1)
        lon = -46.6 + random.uniform(-1, 1)
        alt = random.randint(10000, 35000)
        speed = random.randint(250, 450)
        heading = random.randint(0, 359)
        ver = random.choice([0, 1, 2])
        aircraft_list.append(Aircraft(icao, lat, lon, alt, speed, heading, False, callsign, ver))
        
    # All Surface entities (Airplanes and Vehicles)
    for i in range(len(db_ground)):
        icao, callsign = db_ground[i]
        lat = -23.627 + random.uniform(-0.002, 0.002) # Near Congonhas
        lon = -46.656 + random.uniform(-0.002, 0.002)
        alt = 0
        speed = random.choice([0, 0, 10, 15]) # Parked or taxiing
        heading = random.randint(0, 359)
        ver = random.choice([0, 1, 2])
        aircraft_list.append(Aircraft(icao, lat, lon, alt, speed, heading, True, callsign, ver))

    print(f"Starting ADS-B Simulator... Sending to {UDP_IP}:{UDP_PORT}")
    try:
        while True:
            dt = 0.5 # 2 Hz tick
            for ac in aircraft_list:
                ac.update(dt)
                ac.tick += 1
                
                msgs = []
                if ac.is_surface:
                    msgs.append(generate_surface_position(ac.icao, ac.lat, ac.lon, ac.t_odd, ac.speed, ac.heading))
                    if ac.tick % 10 == 0: # Every 5 seconds
                        msgs.append(encode_callsign(ac.icao, ac.callsign, tc=4, ca=1)) # light surface
                        msgs.append(encode_status(ac.icao, version=ac.adsb_version, subtype=1))
                else:
                    msgs.append(generate_airborne_position(ac.icao, ac.lat, ac.lon, ac.alt, ac.t_odd))
                    msgs.append(generate_airborne_velocity(ac.icao, ac.speed, ac.heading))
                    if ac.tick % 10 == 0: # Every 5 seconds
                        msgs.append(encode_callsign(ac.icao, ac.callsign, tc=4, ca=5)) # heavy airborne
                        msgs.append(encode_status(ac.icao, version=ac.adsb_version, subtype=0))
                
                for msg in msgs:
                    sock.sendto(msg.encode(), (UDP_IP, UDP_PORT))
                    
            time.sleep(dt)
    except KeyboardInterrupt:
        print("\nSimulator stopped.")

if __name__ == "__main__":
    main()
