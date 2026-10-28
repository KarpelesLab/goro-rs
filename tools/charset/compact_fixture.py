#!/usr/bin/env python3
"""Compact a per-line fixture from tools/charset/fixtures/*.php.

Usage: python3 -I tools/charset/compact_fixture.py FULL.txt > COMPACT.txt

The full fixture has one line per probe ("ENC KEY CRC"). The compact one
keeps, per encoding, one line per group of 16 probes: "ENC FIRSTKEY CRC",
where CRC is the crc32 of the group's "KEY CRC\\n" lines. Lines that are not
probes ("ENC empty N") are copied as they are.
"""

import sys
import zlib

groups = []
cur = None
with open(sys.argv[1]) as f:
    for line in f:
        enc, key, val = line.split()
        if key == "empty":
            groups.append((enc, key, val, None))
            cur = None
            continue
        if cur is None or cur[0] != enc or cur[3] == 16:
            cur = [enc, key, b"", 0]
            groups.append(cur)
        cur[2] += ("%s %s\n" % (key, val)).encode()
        cur[3] += 1
for g in groups:
    if g[3] is None:
        print(g[0], g[1], g[2])
    else:
        print(g[0], g[1], "%08x" % zlib.crc32(g[2]))
