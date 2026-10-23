#!/usr/bin/env python3
"""Regenerate crates/goro-ext-date/src/data/{tzdb.bin,tzdb_index.rs,tzmap.rs}.

usage: tools/gen_tzdb.py <php-src>

Reads ext/date/lib/timezonedb.h, timezonemap.h and fallbackmap.h. timelib's
parse_tz.c defines TIMELIB_SUPPORT_SLIM_FILE, so PHP uses the first ("slim")
database of timezonedb.h; that is the one extracted here.
"""
import os
import re
import sys

src = sys.argv[1]
lib = os.path.join(src, 'ext', 'date', 'lib')
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'crates', 'goro-ext-date', 'src', 'data')

db = open(os.path.join(lib, 'timezonedb.h'), encoding='latin-1').read()
slim = db.split('#ifdef TIMELIB_SUPPORT_SLIM_FILE', 1)[1].split('\n#else', 1)[0]
version = re.search(r'timelib_tzdb timezonedb_builtin = \{ "([^"]+)"', db).group(1)

index = re.findall(r'\{ \(char\*\) "([^"]+)"\s*, (0x[0-9A-Fa-f]+) \}', slim)
m = re.search(r'timelib_timezone_db_data_builtin\[(\d+)\] = \{(.*?)\n\};', slim, re.S)
size = int(m.group(1))
body = re.sub(r'/\*.*?\*/', '', m.group(2), flags=re.S)
data = bytes(int(x, 16) for x in re.findall(r'0x([0-9A-Fa-f]{2})', body))
assert len(data) == size, (len(data), size)

with open(os.path.join(out, 'tzdb.bin'), 'wb') as f:
    f.write(data)
with open(os.path.join(out, 'tzdb_index.rs'), 'w') as f:
    f.write('// Generated from php-src ext/date/lib/timezonedb.h by gen_tzdb.py. Do not edit.\n')
    f.write('pub const TZDB_VERSION: &str = "%s";\n' % version)
    f.write('pub static TZDB_INDEX: &[(&str, u32)] = &[\n')
    for name, pos in index:
        f.write('    ("%s", 0x%06X),\n' % (name, int(pos, 16)))
    f.write('];\n')


def read_map(fname):
    txt = open(os.path.join(lib, fname), encoding='latin-1').read()
    rows = []
    for abbr, dst, off, tz in re.findall(r'\{\s*"([^"]*)",\s*(\d+),\s*([-\d* ]+?),\s*(NULL|"[^"]*")\s*\}', txt):
        val = 1
        for part in off.split('*'):
            val *= int(part.strip())
        tz = 'None' if tz == 'NULL' else 'Some(%s)' % tz
        rows.append('    ("%s", %s, %d, %s),\n' % (abbr, dst, val, tz))
    return rows


with open(os.path.join(out, 'tzmap.rs'), 'w') as f:
    f.write('// Generated from php-src ext/date/lib/timezonemap.h + fallbackmap.h by gen_tzdb.py. Do not edit.\n')
    f.write('pub static TIMEZONEMAP: &[(&str, i32, i32, Option<&str>)] = &[\n')
    f.writelines(read_map('timezonemap.h'))
    f.write('];\n')
    f.write('pub static FALLBACKMAP: &[(&str, i32, i32, Option<&str>)] = &[\n')
    f.writelines(read_map('fallbackmap.h'))
    f.write('];\n')
