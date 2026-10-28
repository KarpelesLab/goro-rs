#!/usr/bin/env python3
"""Compile the IANA time zone database into goro-ext-date's embedded table.

Usage: gen_tzdata.py TZDATA_DIR OUTPUT.bin

TZDATA_DIR is an unpacked official tzdata release from
https://www.iana.org/time-zones (tzdataYYYYx.tar.gz). The source files are
read as documented in the zic(8) manual and theory.html of that release; this
script is a small zic-like compiler written from that documentation. The data
is built in the "main" form with backzone (the PACKRATDATA build), which is
what PHP 8.5's own database exposes (observed on PHP 8.5: Europe/Oslo and
America/Montreal carry their own pre-1970 history).

The output is a little-endian binary table read without copying by
crates/goro-ext-date/src/tz/db.rs. Layout (all offsets are byte offsets
from the start of the blob):

  header   "GTZ1", u32 version_off, u32 version_len,
           u32 n_entries, u32 entries_off, u32 n_zones, u32 zones_off,
           u32 strings_off
  entry    (n_entries, sorted by ASCII-lowercased name, 16 bytes each)
           u32 name_off, u16 name_len, u16 zone, u16 flags, u16 pad,
           u32 location_off (0 = none)
             flags bit 0: listed in zone.tab (DateTimeZone::ALL)
             flags bit 1: backward-compatibility link
  zone     (n_zones, 32 bytes each)
           u32 trans_off (i64 array), u32 n_trans, u32 idx_off (u8 array),
           u32 types_off, u32 n_types, u32 tail_off (0 = none),
           u32 pad, u32 pad
  type     8 bytes: i32 utoff, u8 isdst, u8 abbr_len, u16 abbr_off
           (abbr_off relative to strings_off)
  tail     i32 first_year, u32 n_rules, then per rule 12 bytes:
           u8 month (1-12), u8 kind (0 = day of month, 1 = last dow,
           2 = dow >= day, 3 = dow <= day), u8 dow (0 = Sunday), u8 day,
           i32 secs (seconds after local midnight minus the offset that
           turns the rule's AT time into UT), u8 type, 3 pad
  location u8 cc[2], u16 comment_len, i32 lat (1e-5 deg), i32 lon,
           u32 comment_off (relative to strings_off)
"""

import struct
import sys

FILES = [
    "africa", "antarctica", "asia", "australasia", "europe",
    "northamerica", "southamerica", "etcetera", "factory", "backward",
    "backzone",
]

# Explicit transitions are written up to the end of this year (and further
# for zones whose finite rules go beyond it); later instants use the tail
# rules.
CUTOFF_YEAR = 2037

MONTHS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep",
          "oct", "nov", "dec"]
DAYS = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"]

MIN_YEAR = -10 ** 9
MAX_YEAR = 10 ** 9


def days_from_civil(y, m, d):
    y -= m <= 2
    era = (y if y >= 0 else y - 399) // 400
    yoe = y - era * 400
    doy = (153 * (m + (-3 if m > 2 else 9)) + 2) // 5 + d - 1
    doe = yoe * 365 + yoe // 4 - yoe // 100 + doy
    return era * 146097 + doe - 719468


def civil_from_days(z):
    z += 719468
    era = (z if z >= 0 else z - 146096) // 146097
    doe = z - era * 146097
    yoe = (doe - doe // 1460 + doe // 36524 - doe // 146096) // 365
    y = yoe + era * 400
    doy = doe - (365 * yoe + yoe // 4 - yoe // 100)
    mp = (5 * doy + 2) // 153
    d = doy - (153 * mp + 2) // 5 + 1
    m = mp + (3 if mp < 10 else -9)
    return y + (m <= 2), m, d


def is_leap(y):
    return y % 4 == 0 and (y % 100 != 0 or y % 400 == 0)


def month_len(y, m):
    if m == 2:
        return 29 if is_leap(y) else 28
    return [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][m - 1]


def weekday(days):
    return (days + 4) % 7  # 1970-01-01 was a Thursday


def lookup(word, table):
    # The data spells months and days with at least their first three
    # letters ("Mar", "Sun").
    w = word.lower()
    hits = [i for i, t in enumerate(table) if w.startswith(t)]
    if len(hits) != 1:
        raise ValueError("bad name %r" % word)
    return hits[0]


def parse_hms(s):
    """Seconds for [-]h[:mm[:ss]] ("-" means zero)."""
    if s == "-":
        return 0
    neg = s.startswith("-")
    if neg:
        s = s[1:]
    parts = s.split(":")
    total = 0
    mult = 3600
    for p in parts:
        if "." in p:  # fractional seconds are rounded, as zic does
            p = str(round(float(p)))
        total += int(p) * mult
        mult //= 60
    return -total if neg else total


def parse_at(s):
    """(seconds, kind) for an AT/UNTIL time with optional suffix."""
    kind = "w"
    if s and s[-1] in "wsugz":
        kind = s[-1]
        s = s[:-1]
        if kind in "gz":
            kind = "u"
    return parse_hms(s), kind


def parse_day(s):
    """(kind, dow, day): kind 0 = fixed day, 1 = last dow, 2 = >=, 3 = <=."""
    if s.lower().startswith("last"):
        return 1, lookup(s[4:], DAYS), 0
    if ">=" in s:
        a, b = s.split(">=")
        return 2, lookup(a, DAYS), int(b)
    if "<=" in s:
        a, b = s.split("<=")
        return 3, lookup(a, DAYS), int(b)
    return 0, 0, int(s)


def resolve_day(y, m, spec):
    """Day of month for a parsed day spec; may spill into the next/previous
    month (returned as days since the epoch)."""
    kind, dow, day = spec
    if kind == 0:
        return days_from_civil(y, m, day)
    if kind == 1:
        last = days_from_civil(y, m, month_len(y, m))
        return last - (weekday(last) - dow) % 7
    if kind == 2:
        d = days_from_civil(y, m, day)
        return d + (dow - weekday(d)) % 7
    d = days_from_civil(y, m, day)
    return d - (weekday(d) - dow) % 7


class Rule:
    def __init__(self, f):
        # Rule NAME FROM TO - IN ON AT SAVE LETTER/S
        self.name = f[1]
        frm = f[2].lower()
        self.frm = MIN_YEAR if frm.startswith("mi") else int(f[2])
        to = f[3].lower()
        if to.startswith("o"):
            self.to = self.frm
        elif to.startswith("ma"):
            self.to = MAX_YEAR
        else:
            self.to = int(f[3])
        self.month = lookup(f[5], MONTHS) + 1
        self.day = parse_day(f[6])
        self.at, self.at_kind = parse_at(f[7])
        save = f[8]
        self.isdst = None
        if save[-1] in "sd":
            self.isdst = save[-1] == "d"
            save = save[:-1]
        self.save = parse_hms(save)
        if self.isdst is None:
            self.isdst = self.save != 0
        self.letters = "" if f[9] == "-" else f[9]


class ZoneLine:
    def __init__(self, f):
        # STDOFF RULES FORMAT [UNTIL]
        self.stdoff = parse_hms(f[0])
        r = f[1]
        if r == "-":
            self.rules = None
            self.fixed_save = 0
        elif r[0].isdigit() or (r[0] == "-" and len(r) > 1):
            self.rules = None
            self.fixed_save = parse_hms(r)
        else:
            self.rules = r
            self.fixed_save = None
        self.format = f[2]
        self.until = None
        if len(f) > 3:
            y = int(f[3])
            m = lookup(f[4], MONTHS) + 1 if len(f) > 4 else 1
            day = parse_day(f[5]) if len(f) > 5 else (0, 0, 1)
            at, kind = parse_at(f[6]) if len(f) > 6 else (0, "w")
            self.until = (y, m, day, at, kind)


def read_sources(d):
    """Rules, zones and links of the main data plus the backzone entries
    for the names listed in zone.tab (the documented PACKRATDATA=backzone
    PACKRATLIST=zone.tab build; observed on PHP 8.5: Europe/Oslo has its
    own history but America/Montreal is America/Toronto)."""
    rules, zones, links = {}, {}, {}
    order = []
    packrat = set(read_zone_tab(d))
    for name in FILES:
        cur = None
        ignored = False
        with open("%s/%s" % (d, name), encoding="utf-8") as fh:
            for line in fh:
                if name == "backzone":
                    pfx = "#PACKRATLIST zone.tab "
                    if line.startswith(pfx):
                        line = line[len(pfx):]
                    if line.startswith("Zone"):
                        ignored = line.split()[1] not in packrat
                    if ignored and not line.startswith("Rule"):
                        continue
                line = line.split("#", 1)[0]
                if not line.strip():
                    continue
                f = line.split()
                if line[0] in " \t":
                    if cur is None:
                        raise ValueError("continuation without zone")
                    zl = ZoneLine(f)
                    cur.append(zl)
                    if zl.until is None:
                        cur = None
                    continue
                kw = f[0].lower()
                if kw.startswith("ru"):
                    rules.setdefault(f[1], []).append(Rule(f))
                    cur = None
                elif kw.startswith("z"):
                    zname = f[1]
                    lines = []
                    zl = ZoneLine(f[2:])
                    lines.append(zl)
                    # backzone overrides the main data's links; a zone
                    # defined twice keeps its first (main) definition.
                    if zname in zones:
                        if name != "backzone":
                            raise ValueError("duplicate zone " + zname)
                        cur = [] if zl.until is not None else None
                        continue
                    zones[zname] = lines
                    order.append(zname)
                    cur = lines if zl.until is not None else None
                elif kw.startswith("l"):
                    links[f[2]] = f[1]
                    cur = None
                else:
                    raise ValueError("bad line: " + line)
    for zname in zones:
        links.pop(zname, None)
    return rules, zones, links


def fmt_offset(off):
    sign = "-" if off < 0 else "+"
    off = abs(off)
    h, rem = divmod(off, 3600)
    m, s = divmod(rem, 60)
    out = "%s%02d" % (sign, h)
    if m or s:
        out += "%02d" % m
    if s:
        out += "%02d" % s
    return out


def abbr_for(fmt, letters, isdst, utoff):
    if "/" in fmt:
        a, b = fmt.split("/", 1)
        return b if isdst else a
    if "%z" in fmt:
        return fmt.replace("%z", fmt_offset(utoff))
    if "%s" in fmt:
        return fmt.replace("%s", letters)
    return fmt


def until_ut(until, stdoff, save):
    y, m, day, at, kind = until
    local = resolve_day(y, m, day) * 86400 + at
    if kind == "u":
        return local
    if kind == "s":
        return local - stdoff
    return local - stdoff - save


def rule_ut(rule, year, stdoff, save):
    local = resolve_day(year, rule.month, rule.day) * 86400 + rule.at
    if rule.at_kind == "u":
        return local
    if rule.at_kind == "s":
        return local - stdoff
    return local - stdoff - save


def year_of(t):
    return civil_from_days(t // 86400)[0]


def compile_zone(lines, rules, last_year_for_rules):
    """List of (ut, utoff, isdst, abbr); the first has ut None (initial).

    Each zone line contributes a change at its start and one per rule
    occurrence inside it; the list is then reduced as a TZif writer does
    (see optimize)."""
    raw = []
    initial = None
    start = None  # UT start of the current line (None = beginning of time)
    for i, zl in enumerate(lines):
        if zl.rules is None:
            save = zl.fixed_save
            isdst = save != 0
            utoff = zl.stdoff + save
            st = (utoff, isdst, abbr_for(zl.format, "", isdst, utoff))
            if start is None:
                initial = st
            else:
                raw.append((start, st))
            if zl.until is None:
                break
            start = until_ut(zl.until, zl.stdoff, save)
            continue
        rs = rules[zl.rules]
        end_year = last_year_for_rules if zl.until is None \
            else zl.until[0] + 1
        # Every earlier occurrence matters: the save in effect when the
        # line starts comes from the rule set's history (America/Phoenix
        # resumes US war time in 1944 from the 1942 rule).
        first_year = max(min(r.frm for r in rs), 1800) - 1
        save = 0
        start_state = None  # from the last rule before the start
        start_letters_guess = None
        start_added = start is None
        emitted = []
        until = None
        y = first_year
        while y <= end_year and until is None:
            active = [r for r in rs if r.frm <= y <= r.to]
            used = set()
            while True:
                best = None
                for idx, r in enumerate(active):
                    if idx in used:
                        continue
                    t = rule_ut(r, y, zl.stdoff, save)
                    if best is None or t < best[0]:
                        best = (t, idx, r)
                if best is None:
                    break
                t, idx, r = best
                used.add(idx)
                if zl.until is not None:
                    u = until_ut(zl.until, zl.stdoff, save)
                    if t >= u:
                        until = u
                        break
                utoff = zl.stdoff + r.save
                st = (utoff, r.isdst,
                      abbr_for(zl.format, r.letters, r.isdst, utoff))
                if start is not None and t < start:
                    save = r.save
                    start_state = st
                    continue
                if start_state is None and start_letters_guess is None and \
                        r.save == 0:
                    start_letters_guess = st
                save = r.save
                if start is not None and t == start:
                    start_added = True
                emitted.append((t, st))
            y += 1
        if not start_added:
            if start_state is None:
                if start_letters_guess is not None:
                    start_state = start_letters_guess
                else:
                    utoff = zl.stdoff
                    lt = ""
                    for r in sorted(rs, key=lambda r: (r.frm, r.month)):
                        if r.save == 0:
                            lt = r.letters
                            break
                    start_state = (utoff, False,
                                   abbr_for(zl.format, lt, False, utoff))
            raw.append((start, start_state))
        if start is None:
            # the zone's first line is rule based: the initial type is
            # its standard time
            lt = ""
            for r in sorted(rs, key=lambda r: (r.frm, r.month)):
                if r.save == 0:
                    lt = r.letters
                    break
            initial = (zl.stdoff, False,
                       abbr_for(zl.format, lt, False, zl.stdoff))
        raw.extend(emitted)
        if zl.until is None:
            break
        if until is None:
            until = until_ut(zl.until, zl.stdoff, save)
        start = until
    out = [(None,) + initial]
    for t, st in optimize(raw, initial):
        out.append((t,) + st)
    return out


def optimize(raw, initial):
    """Drop the changes a TZif writer drops (observed on PHP 8.5, whose
    data comes from such files): a change that does not move the local
    clock past the previous change's local time is folded into it, and a
    change to the type already in effect is dropped, except for the very
    first change (Europe/Lisbon keeps its 1884 LMT to LMT change)."""
    raw = sorted(raw, key=lambda x: x[0])
    res = []
    for t, st in raw:
        if res:
            before = res[-2][1] if len(res) >= 2 else initial
            if t + res[-1][1][0] <= res[-1][0] + before[0]:
                res[-1] = (res[-1][0], st)
                continue
        if not res or st != res[-1][1]:
            res.append((t, st))
    return res


def tail_rules(lines, rules):
    """The rules that go on forever for the zone's last line, or None."""
    zl = lines[-1]
    if zl.rules is None:
        return None
    rs = [r for r in rules[zl.rules] if r.to == MAX_YEAR]
    if not rs:
        return None
    rs.sort(key=lambda r: (r.month, r.day[2]))
    out = []
    n = len(rs)
    for i, r in enumerate(rs):
        prev = rs[(i - 1) % n]
        if r.at_kind == "u":
            adj = 0
        elif r.at_kind == "s":
            adj = zl.stdoff
        else:
            adj = zl.stdoff + prev.save
        utoff = zl.stdoff + r.save
        out.append((r, r.at - adj, utoff, r.isdst,
                    abbr_for(zl.format, r.letters, r.isdst, utoff)))
    return out


def last_finite_year(lines, rules):
    zl = lines[-1]
    if zl.rules is None:
        return CUTOFF_YEAR
    ys = [r.to for r in rules[zl.rules] if r.to != MAX_YEAR]
    ys += [r.frm for r in rules[zl.rules]]
    return max([CUTOFF_YEAR] + [y for y in ys if y < MAX_YEAR])


def read_zone_tab(d):
    locs = {}
    with open("%s/zone.tab" % d, encoding="utf-8") as fh:
        for line in fh:
            if line.startswith("#") or not line.strip():
                continue
            f = line.rstrip("\n").split("\t")
            cc, coord, name = f[0], f[1], f[2]
            comment = f[3] if len(f) > 3 else ""
            # +DDMM+DDDMM or +DDMMSS+DDDMMSS (ISO 6709)
            for i in range(1, len(coord)):
                if coord[i] in "+-":
                    lat_s, lon_s = coord[:i], coord[i:]
                    break
            locs[name] = (cc, iso6709(lat_s, 2), iso6709(lon_s, 3), comment)
    return locs


def iso6709(s, degdigits):
    sign = -1 if s[0] == "-" else 1
    s = s[1:]
    deg = int(s[:degdigits])
    mins = int(s[degdigits:degdigits + 2])
    secs = int(s[degdigits + 2:degdigits + 4]) if len(s) > degdigits + 2 \
        else 0
    # PHP reports the degrees computed in floating point, truncated to five
    # decimals (observed on PHP 8.5: +404251 is 40.71416, -0740023 is
    # -74.00638 and -01639 is -16.64999, 16 + 39/60 being just below 16.65).
    val = deg + mins / 60.0 + secs / 3600.0
    return sign * int(val * 100000)


def main():
    src, outp = sys.argv[1], sys.argv[2]
    with open("%s/version" % src) as fh:
        version = fh.read().strip()
    rules, zones, links = read_sources(src)
    locs = read_zone_tab(src)

    compiled = {}
    for name, lines in zones.items():
        ly = last_finite_year(lines, rules)
        trans = compile_zone(lines, rules, ly)
        tail = tail_rules(lines, rules)
        compiled[name] = (trans, tail, ly + 1)

    strings = bytearray()
    str_cache = {}

    def intern(s):
        b = s.encode()
        if b not in str_cache:
            pos = strings.find(b) if b else 0
            if pos < 0:
                pos = len(strings)
                strings.extend(b)
            str_cache[b] = pos
        return str_cache[b], len(b)

    zone_names = sorted(zones)
    zone_index = {n: i for i, n in enumerate(zone_names)}
    data = bytearray()
    zone_recs = []

    def align8():
        while len(data) % 8:
            data.append(0)

    for name in zone_names:
        trans, tail, tail_year = compiled[name]
        types = []
        type_idx = {}

        def tid(utoff, isdst, abbr):
            k = (utoff, isdst, abbr)
            if k not in type_idx:
                type_idx[k] = len(types)
                types.append(k)
            return type_idx[k]

        tid(*trans[0][1:])
        idxs = []
        times = []
        for t, utoff, isdst, abbr in trans[1:]:
            times.append(t)
            idxs.append(tid(utoff, isdst, abbr))
        tail_recs = []
        assert not tail or len(tail) <= 4, name
        if tail:
            for r, secs, utoff, isdst, abbr in tail:
                tail_recs.append((r, secs, tid(utoff, isdst, abbr)))
        align8()
        trans_off = len(data)
        for t in times:
            data.extend(struct.pack("<q", t))
        idx_off = len(data)
        data.extend(bytes(idxs))
        while len(data) % 4:
            data.append(0)
        types_off = len(data)
        for utoff, isdst, abbr in types:
            off, ln = intern(abbr)
            data.extend(struct.pack("<iBBH", utoff, 1 if isdst else 0, ln,
                                    off))
        tail_off = 0
        if tail_recs:
            tail_off = len(data)
            data.extend(struct.pack("<iI", tail_year, len(tail_recs)))
            for r, secs, t in tail_recs:
                kind, dow, day = r.day
                data.extend(struct.pack("<BBBBiB3x", r.month, kind, dow, day,
                                        secs, t))
        zone_recs.append((trans_off, len(times), idx_off, types_off,
                          len(types), tail_off))

    loc_offs = {}
    for name, (cc, lat, lon, comment) in sorted(locs.items()):
        while len(data) % 4:
            data.append(0)
        loc_offs[name] = len(data)
        coff, clen = intern(comment)
        data.extend(cc.encode())
        data.extend(struct.pack("<HiiI", clen, lat, lon, coff))

    entries = []
    for name in zones:
        flags = 1 if name in locs else 0
        entries.append((name, zone_index[name], flags))
    for name, target in links.items():
        while target in links:
            target = links[target]
        entries.append((name, zone_index[target], 2 | (1 if name in locs else 0)))
    entries.sort(key=lambda e: e[0].lower())

    header_len = 4 + 4 * 7
    vb = version.encode()
    entries_off = header_len + len(vb)
    entries_off = (entries_off + 7) // 8 * 8
    zones_off = entries_off + 16 * len(entries)
    data_off = zones_off + 32 * len(zone_recs)
    data_off = (data_off + 7) // 8 * 8
    strings_off = data_off + len(data)

    blob = bytearray()
    blob.extend(b"GTZ1")
    blob.extend(struct.pack("<IIIIIII", header_len, len(vb), len(entries),
                            entries_off, len(zone_recs), zones_off,
                            strings_off))
    blob.extend(vb)
    while len(blob) < entries_off:
        blob.append(0)
    for name, z, flags in entries:
        off, ln = intern(name)
        loc = loc_offs.get(name, -data_off) + data_off
        if name not in loc_offs:
            loc = 0
        blob.extend(struct.pack("<IHHHHI", off, ln, z, flags, 0, loc))
    for (trans_off, nt, idx_off, types_off, ntypes, tail_off) in zone_recs:
        blob.extend(struct.pack("<IIIIIIII", trans_off + data_off, nt,
                                idx_off + data_off, types_off + data_off,
                                ntypes, tail_off + data_off if tail_off else 0,
                                0, 0))
    while len(blob) < data_off:
        blob.append(0)
    blob.extend(data)
    assert len(blob) == strings_off
    blob.extend(strings)
    with open(outp, "wb") as fh:
        fh.write(blob)
    print("tzdata %s: %d names, %d zones, %d bytes" %
          (version, len(entries), len(zone_recs), len(blob)))


if __name__ == "__main__":
    main()
