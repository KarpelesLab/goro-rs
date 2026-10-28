#!/usr/bin/env python3
"""Compile CLDR 48 date/time data into goro-ext-intl's embedded tables.

Usage: gen_intl_dates.py CLDR_DIR OUT_BLOB OUT_RS

CLDR_DIR holds the unpacked cldr-json 48.0.0 npm packages (the JSON form of
the Unicode CLDR release, https://github.com/unicode-org/cldr-json):

  CLDR_DIR/dates/package   cldr-dates-full   (ca-gregorian, ca-generic,
                                              timeZoneNames, dateFields)
  CLDR_DIR/cals/package    cldr-cal-*-full   (the other calendars)
  CLDR_DIR/names/package   cldr-localenames-full (territories)
  CLDR_DIR/core/package    cldr-core         (supplemental data)
  CLDR_DIR/bcp47/package   cldr-bcp47        (time zone ids)

The data is read as the LDML specification (UTS #35) describes it. Two
outputs:

OUT_BLOB (crates/goro-ext-intl/data/dates.bin.z), per-locale strings, stored
as a raw DEFLATE stream; crates/goro-ext-intl/src/datefmt/data.rs inflates
it once and reads it in place. Little-endian layout of the inflated blob:

  header   "GID1", u32 pool_off, u32 pool_len, u32 keys_off, u32 n_keys,
           u32 recs_off, u32 n_recs, u32 locs_off, u32 n_locs, u32 n_sections
  pool     strings: u16 byte length, then UTF-8 bytes; a string reference is
           the offset of its length field from pool_off
  keys     n_keys u32 string references, sorted by the strings' bytes
  recs     n_recs u32 offsets (from the blob start) of records
  record   u32 base record (0xFFFFFFFF: none), u32 n, n u16 key indexes
           (ascending), padding to 4 bytes, n u32 value references
           (0xFFFFFFFF: the key is absent here, do not look in the base)
  locs     n_locs entries sorted by locale id: u32 id reference, then
           n_sections u32 record indexes (0xFFFFFFFF: no data)

A record holds the fully resolved data of one section of one locale as a
difference from its base record (the parent locale's same section, or the
same locale's Gregorian calendar for the other calendars), so a lookup walks
the base chain. Sections: SECTIONS below.

OUT_RS (crates/goro-ext-intl/src/datefmt/supplemental.rs): supplemental
tables as Rust statics (week data, time data, calendar preferences,
metazones, primary zones, Windows zone mappings, BCP 47 time zone ids,
Japanese eras, locale lists).
"""

import json
import os
import struct
import sys

CALS = [
    "gregorian", "generic", "buddhist", "chinese", "coptic", "dangi",
    "ethiopic", "ethiopic-amete-alem", "hebrew", "indian", "islamic",
    "islamic-civil", "islamic-rgsa", "islamic-tbla", "islamic-umalqura",
    "japanese", "persian", "roc",
]
SECTIONS = CALS + ["tz", "fields", "territories"]

SKIP_CAL_KEYS = ("intervalFormats", "dateSkeletons", "timeSkeletons")


def load(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def flatten(obj, prefix, out):
    for k, v in obj.items():
        if "-alt-ascii" in k:
            continue
        key = prefix + k if not prefix else prefix + "/" + k
        if isinstance(v, dict):
            flatten(v, key, out)
        elif isinstance(v, list):
            out[key] = " ".join(v)
        else:
            out[key] = str(v)


def cal_data(root, loc, cal):
    if cal in ("gregorian", "generic"):
        p = os.path.join(root, "dates/package/main", loc, "ca-%s.json" % cal)
    else:
        p = os.path.join(root, "cals/package/main", loc, "ca-%s.json" % cal)
    if not os.path.exists(p):
        return None
    d = load(p)["main"][loc]["dates"]["calendars"][cal]
    out = {}
    for k, v in d.items():
        if k in SKIP_CAL_KEYS:
            continue
        if isinstance(v, dict):
            sub = {kk: vv for kk, vv in v.items() if kk != "intervalFormats"}
            flatten(sub, k, out)
        else:
            out[k] = str(v)
    return out


def tz_data(root, loc):
    p = os.path.join(root, "dates/package/main", loc, "timeZoneNames.json")
    d = load(p)["main"][loc]["dates"]["timeZoneNames"]
    out = {}
    for k, v in d.items():
        if k == "zone":
            walk_zones(v, "", out)
        elif k == "metazone":
            for mz, names in v.items():
                flat = {}
                flatten(names, "", flat)
                for nk, nv in flat.items():
                    out["m:" + mz + "|" + nk] = nv
        else:
            out[k] = str(v)
    return out


def walk_zones(obj, prefix, out):
    for k, v in obj.items():
        zid = k if not prefix else prefix + "/" + k
        if isinstance(v, dict) and v.get("_type") == "zone":
            flat = {}
            flatten({kk: vv for kk, vv in v.items() if kk != "_type"}, "", flat)
            for nk, nv in flat.items():
                out["z:" + zid + "|" + nk] = nv
        elif isinstance(v, dict):
            walk_zones(v, zid, out)


def fields_data(root, loc):
    p = os.path.join(root, "dates/package/main", loc, "dateFields.json")
    d = load(p)["main"][loc]["dates"]["fields"]
    out = {}
    for k in ("day", "day-short", "day-narrow"):
        if k in d:
            for kk, vv in d[k].items():
                if kk.startswith("relative-type"):
                    out[k + "/" + kk] = vv
    return out


def terr_data(root, loc):
    p = os.path.join(root, "names/package/main", loc, "territories.json")
    if not os.path.exists(p):
        return None
    d = load(p)["main"][loc]["localeDisplayNames"]["territories"]
    return {k: v for k, v in d.items() if "-alt-" not in k}


class Pool:
    def __init__(self):
        self.buf = bytearray()
        self.refs = {}

    def ref(self, s):
        r = self.refs.get(s)
        if r is None:
            b = s.encode("utf-8")
            assert len(b) < 65536
            r = len(self.buf)
            self.buf += struct.pack("<H", len(b)) + b
            self.refs[s] = r
        return r


def icu_id(loc):
    return loc.replace("-", "_")


def main():
    root, out_blob, out_rs = sys.argv[1:4]
    core = os.path.join(root, "core/package")
    avail = load(os.path.join(core, "availableLocales.json"))["availableLocales"]["full"]
    default_content = load(os.path.join(core, "defaultContent.json"))["defaultContent"]
    parents = load(os.path.join(core, "supplemental/parentLocales.json"))[
        "supplemental"]["parentLocales"]["parentLocale"]
    locs = sorted(avail)

    def parent(loc):
        if loc == "und":
            return None
        if loc in parents:
            return parents[loc]
        if "-" in loc:
            p = loc.rsplit("-", 1)[0]
            # A script subtag that is not the language's default makes the
            # parent root (UTS #35 "Parent Locales"); cldr-json marks those
            # in parentLocales, so plain truncation is right here.
            return p
        return "und"

    # Resolved data per (locale, section).
    data = {}
    for loc in locs:
        for cal in CALS:
            d = cal_data(root, loc, cal)
            if d is not None:
                data[(loc, cal)] = d
        data[(loc, "tz")] = tz_data(root, loc)
        data[(loc, "fields")] = fields_data(root, loc)
        t = terr_data(root, loc)
        if t is not None:
            data[(loc, "territories")] = t

    pool = Pool()
    keys = sorted({k for d in data.values() for k in d})
    assert len(keys) < 65536, len(keys)
    key_idx = {k: i for i, k in enumerate(keys)}
    for k in keys:
        pool.ref(k)

    records = []  # (base, [(key_idx, value_ref)])
    rec_index = {}  # (loc, section) -> record index
    rec_dedup = {}

    def diff(d, base):
        out = []
        for k, v in d.items():
            if base is None or base.get(k) != v:
                out.append((key_idx[k], pool.ref(v)))
        if base is not None:
            for k in base:
                if k not in d:
                    out.append((key_idx[k], 0xFFFFFFFF))
        out.sort()
        return out

    def resolve_order(loc):
        chain = []
        while loc is not None:
            chain.append(loc)
            loc = parent(loc)
        return chain

    def build(loc, sec):
        if (loc, sec) in rec_index:
            return rec_index[(loc, sec)]
        d = data.get((loc, sec))
        if d is None:
            return None
        # candidate bases: the parent locale's section, or for the
        # non-Gregorian calendars the same locale's Gregorian data
        cands = []
        p = parent(loc)
        while p is not None and (p, sec) not in data:
            p = parent(p)
        if p is not None:
            cands.append((p, sec))
        if sec in CALS and sec != "gregorian" and (loc, "gregorian") in data:
            cands.append((loc, "gregorian"))
        best = None
        for c in cands:
            ents = diff(d, data[c])
            if best is None or len(ents) < len(best[1]):
                best = (c, ents)
        if best is None:
            base_rec = 0xFFFFFFFF
            ents = diff(d, None)
        else:
            base_rec = build(*best[0])
            ents = best[1]
        sig = (base_rec, tuple(ents))
        r = rec_dedup.get(sig)
        if r is None:
            r = len(records)
            records.append(sig)
            rec_dedup[sig] = r
        rec_index[(loc, sec)] = r
        return r

    loc_rows = []
    for loc in locs:
        row = [build(loc, s) for s in SECTIONS]
        loc_rows.append((icu_id(loc), [0xFFFFFFFF if r is None else r for r in row]))
    loc_rows.sort(key=lambda x: x[0].encode())
    for name, _ in loc_rows:
        pool.ref(name)

    # Assemble.
    hdr_len = 4 + 4 * 9
    pool_off = hdr_len
    blob = bytearray(hdr_len)
    blob += pool.buf
    while len(blob) % 4:
        blob.append(0)
    keys_off = len(blob)
    for k in keys:
        blob += struct.pack("<I", pool.ref(k))
    recs_off = len(blob)
    blob += bytes(4 * len(records))
    rec_offs = []
    for base, ents in records:
        rec_offs.append(len(blob))
        blob += struct.pack("<II", base, len(ents))
        for k, _ in ents:
            blob += struct.pack("<H", k)
        while len(blob) % 4:
            blob.append(0)
        for _, v in ents:
            blob += struct.pack("<I", v)
    for i, o in enumerate(rec_offs):
        struct.pack_into("<I", blob, recs_off + 4 * i, o)
    locs_off = len(blob)
    for name, row in loc_rows:
        blob += struct.pack("<I", pool.ref(name))
        for r in row:
            blob += struct.pack("<I", r)
    struct.pack_into("<4s9I", blob, 0, b"GID1", pool_off, len(pool.buf), keys_off,
                     len(keys), recs_off, len(records), locs_off, len(loc_rows),
                     len(SECTIONS))
    # Stored as a raw DEFLATE stream (RFC 1951), inflated on first use.
    import zlib
    c = zlib.compressobj(9, zlib.DEFLATED, -15, 9)
    packed = c.compress(bytes(blob)) + c.flush()
    with open(out_blob, "wb") as f:
        f.write(packed)
    print("blob: %d bytes, %d strings bytes, %d keys, %d records, %d locales" % (
        len(blob), len(pool.buf), len(keys), len(records), len(loc_rows)))

    write_rs(root, out_rs, locs, default_content, parents)


def rs_str(s):
    return json.dumps(s, ensure_ascii=False)


def write_rs(root, out_rs, locs, default_content, parents):
    core = os.path.join(root, "core/package/supplemental")
    o = []
    o.append("//! Supplemental CLDR 48 data for the date/time cluster, generated by")
    o.append("//! tools/gen_intl_dates.py from cldr-json 48.0.0 (cldr-core, cldr-bcp47).")
    o.append("//! Do not edit.")
    o.append("")
    o.append("#![allow(clippy::all)]")
    o.append("")
    o.append("/// The sections of a locale row in dates.bin, in order.")
    o.append("pub const SECTIONS: &[&str] = &[%s];" % ", ".join(rs_str(s) for s in SECTIONS))
    o.append("")
    o.append("/// CLDR's locales (\"full\" set), ICU spelling, sorted.")
    o.append("pub static LOCALES: &[&str] = &[%s];" % ", ".join(
        rs_str(icu_id(l)) for l in sorted(locs, key=lambda x: icu_id(x).encode())))
    o.append("")
    o.append("/// Default-content locales (no data of their own; UTS #35).")
    o.append("pub static DEFAULT_CONTENT: &[&str] = &[%s];" % ", ".join(
        rs_str(icu_id(l)) for l in sorted(default_content, key=lambda x: icu_id(x).encode())))
    o.append("")
    o.append("/// Explicit parent locales (supplemental parentLocales).")
    o.append("pub static PARENT_LOCALES: &[(&str, &str)] = &[%s];" % ", ".join(
        "(%s, %s)" % (rs_str(icu_id(k)), rs_str(icu_id(v)))
        for k, v in sorted(parents.items(), key=lambda kv: icu_id(kv[0]).encode())))
    o.append("")

    days = {"sun": 1, "mon": 2, "tue": 3, "wed": 4, "thu": 5, "fri": 6, "sat": 7}
    wd = load(os.path.join(core, "weekData.json"))["supplemental"]["weekData"]
    terrs = sorted(set(wd["minDays"]) | set(wd["firstDay"]) | set(wd["weekendStart"])
                   | set(wd["weekendEnd"]))
    o.append("/// Week data per territory: (territory, first day, minimal days,")
    o.append("/// weekend start, weekend end); days 1 = Sunday … 7 = Saturday; 0 =")
    o.append("/// the \"001\" value. Sorted by territory.")
    o.append("pub static WEEK_DATA: &[(&str, u8, u8, u8, u8)] = &[")
    for t in terrs:
        fd = days.get(wd["firstDay"].get(t, ""), 0)
        md = int(wd["minDays"].get(t, "0"))
        ws = days.get(wd["weekendStart"].get(t, ""), 0)
        we = days.get(wd["weekendEnd"].get(t, ""), 0)
        o.append("    (%s, %d, %d, %d, %d)," % (rs_str(t), fd, md, ws, we))
    o.append("];")
    o.append("")

    td = load(os.path.join(core, "timeData.json"))["supplemental"]["timeData"]
    o.append("/// Hour cycle data: (region or locale, allowed, preferred). Sorted.")
    o.append("pub static TIME_DATA: &[(&str, &str, &str)] = &[")
    for k in sorted(td):
        o.append("    (%s, %s, %s)," % (rs_str(k), rs_str(td[k]["_allowed"]), rs_str(td[k]["_preferred"])))
    o.append("];")
    o.append("")

    cp = load(os.path.join(core, "calendarPreferenceData.json"))["supplemental"]["calendarPreferenceData"]
    o.append("/// Calendar preferences per territory. Sorted.")
    o.append("pub static CALENDAR_PREFS: &[(&str, &str)] = &[")
    for k in sorted(cp):
        v = cp[k]
        if isinstance(v, list):
            v = " ".join(v)
        o.append("    (%s, %s)," % (rs_str(k), rs_str(v)))
    o.append("];")
    o.append("")

    cd = load(os.path.join(core, "calendarData.json"))["supplemental"]["calendarData"]
    eras = cd["japanese"]["eras"]
    o.append("/// Japanese eras: (start year, month, day), by era index.")
    o.append("pub static JAPANESE_ERAS: &[(i32, u8, u8)] = &[")
    for i in sorted(eras, key=int):
        s = eras[i]["_start"]
        neg = s.startswith("-")
        y, m, d = (s[1:] if neg else s).split("-")
        o.append("    (%d, %d, %d)," % (-int(y) if neg else int(y), int(m), int(d)))
    o.append("];")
    o.append("")

    mz = load(os.path.join(core, "metaZones.json"))["supplemental"]["metaZones"]
    periods = []

    def walk(obj, prefix):
        for k, v in obj.items():
            zid = k if not prefix else prefix + "/" + k
            if isinstance(v, list):
                periods.append((zid, v))
            else:
                walk(v, zid)
    walk(mz["metazoneInfo"]["timezone"], "")
    periods.sort(key=lambda x: x[0].encode())

    def ts(s):
        # "YYYY-MM-DD HH:MM" UTC -> Unix ms
        import datetime
        dt = datetime.datetime.strptime(s, "%Y-%m-%d %H:%M").replace(tzinfo=datetime.timezone.utc)
        return int(dt.timestamp()) * 1000

    o.append("/// Metazone usage per zone: (zone, [(metazone, from ms, to ms)]);")
    o.append("/// i64::MIN / i64::MAX are open ends. Sorted by zone.")
    o.append("pub static METAZONE_PERIODS: &[(&str, &[(&str, i64, i64)])] = &[")
    for zid, lst in periods:
        items = []
        for e in lst:
            u = e["usesMetazone"]
            f = ts(u["_from"]) if "_from" in u else None
            t = ts(u["_to"]) if "_to" in u else None
            items.append("(%s, %s, %s)" % (rs_str(u["_mzone"]),
                                           "i64::MIN" if f is None else str(f),
                                           "i64::MAX" if t is None else str(t)))
        o.append("    (%s, &[%s])," % (rs_str(zid), ", ".join(items)))
    o.append("];")
    o.append("")
    o.append("/// Metazone golden zones: (metazone, territory, zone). Sorted.")
    o.append("pub static METAZONE_MAP: &[(&str, &str, &str)] = &[")
    mm = []
    for e in mz["metazones"]:
        m = e["mapZone"]
        mm.append((m["_other"], m["_territory"], m["_type"]))
    for m in sorted(mm, key=lambda x: (x[0].encode(), x[1].encode())):
        o.append("    (%s, %s, %s)," % tuple(rs_str(x) for x in m))
    o.append("];")
    o.append("")

    pz = load(os.path.join(core, "primaryZones.json"))["supplemental"]["primaryZones"]
    o.append("/// Primary zones of countries with several zones. Sorted.")
    o.append("pub static PRIMARY_ZONES: &[(&str, &str)] = &[%s];" % ", ".join(
        "(%s, %s)" % (rs_str(k), rs_str(pz[k])) for k in sorted(pz)))
    o.append("")

    wz = load(os.path.join(core, "windowsZones.json"))["supplemental"]["windowsZones"]["mapTimezones"]
    o.append("/// Windows zone mappings: (windows id, territory, zones), in CLDR order.")
    o.append("pub static WINDOWS_ZONES: &[(&str, &str, &str)] = &[")
    for e in wz:
        m = e["mapZone"]
        o.append("    (%s, %s, %s)," % (rs_str(m["_other"]), rs_str(m["_territory"]), rs_str(m["_type"])))
    o.append("];")
    o.append("")

    tz = load(os.path.join(root, "bcp47/package/bcp47/timezone.json"))["keyword"]["u"]["tz"]
    o.append("/// BCP 47 time zone ids: (short id, aliases (the first is CLDR's")
    o.append("/// canonical id), IANA id if different, deprecated?, preferred). Sorted.")
    o.append("pub static BCP47_ZONES: &[(&str, &str, &str, bool, &str)] = &[")
    for k in sorted(tz):
        if k.startswith("_"):
            continue
        v = tz[k]
        o.append("    (%s, %s, %s, %s, %s)," % (
            rs_str(k), rs_str(v.get("_alias", "")), rs_str(v.get("_iana", "")),
            "true" if v.get("_deprecated") else "false", rs_str(v.get("_preferred", ""))))
    o.append("];")
    o.append("")

    with open(out_rs, "w", encoding="utf-8") as f:
        f.write("\n".join(o))


if __name__ == "__main__":
    main()
