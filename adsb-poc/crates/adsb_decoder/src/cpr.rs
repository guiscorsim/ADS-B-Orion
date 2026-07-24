//! Compact Position Reporting (CPR) — airborne + surface decode.

const NZ: f64 = 15.0;
const CPR_MAX: f64 = 131_072.0;

/// Number of longitude zones NL(lat) per ICAO Doc 9871.
pub fn nl(lat: f64) -> i32 {
    let lat = lat.abs();
    if lat >= 87.0 {
        return 1;
    }
    if lat < 1e-6 {
        return 59;
    }
    let a =
        1.0 - (1.0 - (std::f64::consts::PI / (2.0 * NZ)).cos()) / lat.to_radians().cos().powi(2);
    if a <= 0.0 {
        return 1;
    }
    (2.0 * std::f64::consts::PI / a.acos()).floor() as i32
}

/// Positive modulo for CPR zone indices.
fn cpr_mod(a: f64, n: f64) -> f64 {
    let mut r = a % n;
    if r < 0.0 {
        r += n;
    }
    r
}

/// Decode airborne CPR from even/odd pair. `even_newer` selects which frame
/// supplies the final lat/lon (ICAO: use the more recent of the two).
pub fn decode_airborne(
    lat_cpr_even: u32,
    lon_cpr_even: u32,
    lat_cpr_odd: u32,
    lon_cpr_odd: u32,
    even_newer: bool,
) -> Option<(f64, f64)> {
    let dlat0 = 360.0 / (4.0 * NZ);
    let dlat1 = 360.0 / (4.0 * NZ - 1.0);

    let j = ((59.0 * lat_cpr_even as f64 - 60.0 * lat_cpr_odd as f64) / CPR_MAX + 0.5).floor();

    let mut lat0 = dlat0 * (cpr_mod(j, 60.0) + lat_cpr_even as f64 / CPR_MAX);
    let mut lat1 = dlat1 * (cpr_mod(j, 59.0) + lat_cpr_odd as f64 / CPR_MAX);
    if lat0 >= 270.0 {
        lat0 -= 360.0;
    }
    if lat1 >= 270.0 {
        lat1 -= 360.0;
    }

    // Latitude zone discontinuity — refuse rather than invent a fix.
    if nl(lat0) != nl(lat1) {
        return None;
    }

    let lat = if even_newer { lat0 } else { lat1 };
    if !(-90.0..=90.0).contains(&lat) {
        return None;
    }

    let nl_val = nl(lat) as f64;
    let m = ((lon_cpr_even as f64 * (nl_val - 1.0) - lon_cpr_odd as f64 * nl_val) / CPR_MAX + 0.5)
        .floor();

    let (ni, lon_cpr) = if even_newer {
        ((nl_val - 0.0).max(1.0), lon_cpr_even as f64)
    } else {
        ((nl_val - 1.0).max(1.0), lon_cpr_odd as f64)
    };
    let dlon = 360.0 / ni;
    let mut lon = dlon * (cpr_mod(m, ni) + lon_cpr / CPR_MAX);
    if lon >= 180.0 {
        lon -= 360.0;
    }
    if !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    Some((lat, lon))
}

/// Decode airborne CPR relative to a known nearby position (local/relative CPR).
/// Used after a global even/odd fix so single frames can update the track.
pub fn decode_airborne_local(
    lat_cpr: u32,
    lon_cpr: u32,
    odd: bool,
    ref_lat: f64,
    ref_lon: f64,
) -> Option<(f64, f64)> {
    let dlat = if odd {
        360.0 / (4.0 * NZ - 1.0)
    } else {
        360.0 / (4.0 * NZ)
    };
    let j = (ref_lat / dlat).floor()
        + (0.5 + cpr_mod(ref_lat, dlat) / dlat - lat_cpr as f64 / CPR_MAX).floor();
    let mut lat = dlat * (j + lat_cpr as f64 / CPR_MAX);
    if lat >= 270.0 {
        lat -= 360.0;
    }
    if !(-90.0..=90.0).contains(&lat) {
        return None;
    }

    let nl_val = nl(lat) as f64;
    let ni = if odd {
        (nl_val - 1.0).max(1.0)
    } else {
        nl_val.max(1.0)
    };
    let dlon = 360.0 / ni;
    let m = (ref_lon / dlon).floor()
        + (0.5 + cpr_mod(ref_lon, dlon) / dlon - lon_cpr as f64 / CPR_MAX).floor();
    let mut lon = dlon * (m + lon_cpr as f64 / CPR_MAX);
    let mut best = lon;
    let mut best_d = (lon - ref_lon).abs();
    for dm in [-1.0_f64, 1.0] {
        let cand = dlon * (m + dm + lon_cpr as f64 / CPR_MAX);
        let d = (cand - ref_lon).abs();
        if d < best_d {
            best_d = d;
            best = cand;
        }
    }
    lon = best;
    if lon < -180.0 {
        lon += 360.0;
    }
    if lon >= 180.0 {
        lon -= 360.0;
    }
    if !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    if haversine_nm(lat, lon, ref_lat, ref_lon) > 180.0 {
        return None;
    }
    Some((lat, lon))
}

/// Surface CPR global decode relative to a known approximate location
/// (receiver or last airborne fix). Returns None if the decoded cell is
/// implausibly far from the reference (>180 NM).
pub fn decode_surface(
    lat_cpr_even: u32,
    lon_cpr_even: u32,
    lat_cpr_odd: u32,
    lon_cpr_odd: u32,
    even_newer: bool,
    ref_lat: f64,
    ref_lon: f64,
) -> Option<(f64, f64)> {
    let dlat0 = 90.0 / (4.0 * NZ);
    let dlat1 = 90.0 / (4.0 * NZ - 1.0);

    let j = ((59.0 * lat_cpr_even as f64 - 60.0 * lat_cpr_odd as f64) / CPR_MAX + 0.5).floor();

    let mut lat0 = dlat0 * (cpr_mod(j, 60.0) + lat_cpr_even as f64 / CPR_MAX);
    let mut lat1 = dlat1 * (cpr_mod(j, 59.0) + lat_cpr_odd as f64 / CPR_MAX);

    // Surface latitudes are in [0, 90); pick the hemisphere matching the ref.
    if ref_lat < 0.0 {
        lat0 = -lat0;
        lat1 = -lat1;
    }

    if nl(lat0) != nl(lat1) {
        return None;
    }

    let lat = if even_newer { lat0 } else { lat1 };
    if !(-90.0..=90.0).contains(&lat) {
        return None;
    }

    let nl_val = nl(lat) as f64;
    let m = ((lon_cpr_even as f64 * (nl_val - 1.0) - lon_cpr_odd as f64 * nl_val) / CPR_MAX + 0.5)
        .floor();

    let (ni, lon_cpr) = if even_newer {
        (nl_val.max(1.0), lon_cpr_even as f64)
    } else {
        ((nl_val - 1.0).max(1.0), lon_cpr_odd as f64)
    };
    let dlon = 90.0 / ni;
    let mut lon = dlon * (cpr_mod(m, ni) + lon_cpr / CPR_MAX);

    // Resolve 90° longitude ambiguity toward the reference.
    lon = resolve_surface_lon(lon, ref_lon, dlon);

    if !(-180.0..=180.0).contains(&lon) || !(-90.0..=90.0).contains(&lat) {
        return None;
    }

    if haversine_nm(lat, lon, ref_lat, ref_lon) > 180.0 {
        return None;
    }
    Some((lat, lon))
}

fn resolve_surface_lon(lon: f64, ref_lon: f64, dlon: f64) -> f64 {
    // Candidates: lon + k*4*dlon covering ±180° around the reference.
    let mut best = lon;
    let mut best_d = f64::INFINITY;
    for k in -2i32..=2 {
        let mut cand = lon + f64::from(k) * 4.0 * dlon;
        if cand < -180.0 {
            cand += 360.0;
        }
        if cand >= 180.0 {
            cand -= 360.0;
        }
        let d = (cand - ref_lon).abs().min(360.0 - (cand - ref_lon).abs());
        if d < best_d {
            best_d = d;
            best = cand;
        }
    }
    best
}

/// Great-circle distance in nautical miles (CPR sanity checks).
pub fn haversine_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r_nm = 3440.065; // Earth radius in nautical miles
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * r_nm * a.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nl_equator_and_pole() {
        assert_eq!(nl(0.0), 59);
        assert_eq!(nl(87.0), 1);
        assert_eq!(nl(90.0), 1);
    }

    #[test]
    fn airborne_known_pair_from_me() {
        // CPR from 8D40621D58C382D690C8AC2863A7 (even) /
        // 8D40621D58C386435CC412692AD6 (odd) → ~52.257 / 3.919 (even-newer)
        let ye = 93_000;
        let xe = 51_372;
        let yo = 74_158;
        let xo = 50_194;
        let (lat, lon) = decode_airborne(ye, xe, yo, xo, true).expect("cpr");
        assert!((lat - 52.257).abs() < 0.01, "lat={lat}");
        assert!((lon - 3.919).abs() < 0.01, "lon={lon}");
    }

    #[test]
    fn airborne_local_near_global() {
        let ye = 93_000;
        let xe = 51_372;
        let yo = 74_158;
        let xo = 50_194;
        let (lat, lon) = decode_airborne(ye, xe, yo, xo, true).expect("global");
        let (lat2, lon2) = decode_airborne_local(ye, xe, false, lat, lon).expect("local");
        assert!((lat2 - lat).abs() < 1e-6);
        assert!((lon2 - lon).abs() < 1e-6);
    }

    #[test]
    fn surface_cpr_near_ref() {
        // Encoded for ~52.3N 4.8E (surface CPR); far ref must reject, near ref must decode.
        let ref_lat = 52.3;
        let ref_lon = 4.8;
        let lat_e = 0x1_BBBC;
        let lon_e = 0x1_D70A;
        let lat_o = 0x0_9234;
        let lon_o = 0x1_BBBC;
        assert!(decode_surface(lat_e, lon_e, lat_o, lon_o, true, -33.9, 151.2).is_none());
        let (lat, lon) =
            decode_surface(lat_e, lon_e, lat_o, lon_o, true, ref_lat, ref_lon).expect("near ref");
        assert!(haversine_nm(lat, lon, ref_lat, ref_lon) < 1.0);
        assert!((lat - ref_lat).abs() < 0.01, "lat={lat}");
        assert!((lon - ref_lon).abs() < 0.01, "lon={lon}");
    }
}
