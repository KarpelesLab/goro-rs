//! Port of timelib astro.c

use super::tm2unixtime::update_ts;
use super::Time;
use std::f64::consts::PI;

const RADEG: f64 = 180.0 / PI;
const DEGRAD: f64 = PI / 180.0;
const INV360: f64 = 1.0 / 360.0;

fn sind(x: f64) -> f64 {
    (x * DEGRAD).sin()
}
fn cosd(x: f64) -> f64 {
    (x * DEGRAD).cos()
}
fn atan2d(y: f64, x: f64) -> f64 {
    RADEG * y.atan2(x)
}
fn acosd(x: f64) -> f64 {
    RADEG * x.acos()
}

fn astro_revolution(x: f64) -> f64 {
    x - 360.0 * (x * INV360).floor()
}

fn astro_rev180(x: f64) -> f64 {
    x - 360.0 * (x * INV360 + 0.5).floor()
}

fn astro_gmst0(d: f64) -> f64 {
    astro_revolution((180.0 + 356.0470 + 282.9404) + (0.9856002585 + 4.70935E-5) * d)
}

fn astro_sunpos(d: f64) -> (f64, f64) {
    let m = astro_revolution(356.0470 + 0.9856002585 * d);
    let w = 282.9404 + 4.70935E-5 * d;
    let e = 0.016709 - 1.151E-9 * d;
    let ee = m + e * RADEG * sind(m) * (1.0 + e * cosd(m));
    let x = cosd(ee) - e;
    let y = (1.0 - e * e).sqrt() * sind(ee);
    let r = (x * x + y * y).sqrt();
    let v = atan2d(y, x);
    let mut lon = v + w;
    if lon >= 360.0 {
        lon -= 360.0;
    }
    (lon, r)
}

fn astro_sun_ra_dec(d: f64) -> (f64, f64, f64) {
    let (lon, r) = astro_sunpos(d);
    let x = r * cosd(lon);
    let y0 = r * sind(lon);
    let obl_ecl = 23.4393 - 3.563E-7 * d;
    let z = y0 * sind(obl_ecl);
    let y = y0 * cosd(obl_ecl);
    let ra = atan2d(y, x);
    let dec = atan2d(z, (x * x + y * y).sqrt());
    (ra, dec, r)
}

pub fn ts_to_julianday(ts: i64) -> f64 {
    let mut tmp = ts as f64;
    tmp /= 86400.0;
    tmp += 2440587.5;
    tmp
}

pub fn ts_to_j2000(ts: i64) -> f64 {
    ts_to_julianday(ts) - 2451545.0
}

pub struct RiseSet {
    pub rc: i32,
    pub h_rise: f64,
    pub h_set: f64,
    pub ts_rise: i64,
    pub ts_set: i64,
    pub ts_transit: i64,
}

/// timelib_astro_rise_set_altitude
pub fn rise_set_altitude(t_loc: &mut Time, lon: f64, lat: f64, altit_in: f64, upper_limb: bool) -> RiseSet {
    let mut altit = altit_in;
    let old_sse = t_loc.sse;
    t_loc.h = 12;
    t_loc.i = 0;
    t_loc.s = 0;
    update_ts(t_loc, None);

    let mut t_utc = Time::new();
    t_utc.y = t_loc.y;
    t_utc.m = t_loc.m;
    t_utc.d = t_loc.d;
    t_utc.h = 0;
    t_utc.i = 0;
    t_utc.s = 0;
    update_ts(&mut t_utc, None);

    let timestamp = t_utc.sse;
    let d = ts_to_j2000(timestamp) + 2.0 - lon / 360.0;
    let sidtime = astro_revolution(astro_gmst0(d) + 180.0 + lon);
    let (s_ra, sdec, sr) = astro_sun_ra_dec(d);
    let tsouth = 12.0 - astro_rev180(sidtime - s_ra) / 15.0;
    let sradius = 0.2666 / sr;
    if upper_limb {
        altit -= sradius;
    }
    let mut res = RiseSet { rc: 0, h_rise: 0.0, h_set: 0.0, ts_rise: 0, ts_set: 0, ts_transit: 0 };
    let cost = (sind(altit) - sind(lat) * sind(sdec)) / (cosd(lat) * cosd(sdec));
    res.ts_transit = (t_utc.sse as f64 + tsouth * 3600.0) as i64;
    if cost >= 1.0 {
        res.rc = -1;
        let v = (t_utc.sse as f64 + tsouth * 3600.0) as i64;
        res.ts_rise = v;
        res.ts_set = v;
    } else if cost <= -1.0 {
        res.rc = 1;
        res.ts_rise = t_loc.sse - 12 * 3600;
        res.ts_set = t_loc.sse + 12 * 3600;
    } else {
        let t = acosd(cost) / 15.0;
        res.ts_rise = (((tsouth - t) * 3600.0) + t_utc.sse as f64) as i64;
        res.ts_set = (((tsouth + t) * 3600.0) + t_utc.sse as f64) as i64;
        res.h_rise = tsouth - t;
        res.h_set = tsouth + t;
    }
    t_loc.sse = old_sse;
    res
}
