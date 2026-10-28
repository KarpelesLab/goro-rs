#!/usr/bin/env python3
"""Generates crates/goro-ext-intl/src/data/uts46.txt from the Unicode IDNA
mapping table (UTS #46, IdnaMappingTable.txt, Unicode 17.0.0, published
at https://www.unicode.org/Public/17.0.0/idna/).

usage: gen_intl_uts46.py IdnaMappingTable.txt > uts46.txt

One line per range start: "start status [mapping]" in hex, status one of
v (valid), m (mapped), i (ignored), d (disallowed), x (deviation); the
mapping is space-separated code points (deviations may map to nothing).
Consecutive code points with the same status and no mapping are merged.
"""
import sys

CODE = {"valid": "v", "mapped": "m", "ignored": "i", "disallowed": "d", "deviation": "x"}


def main():
    entries = []
    for line in open(sys.argv[1], encoding="utf-8"):
        body = line.split("#", 1)[0].strip()
        if not body:
            continue
        f = [x.strip() for x in body.split(";")]
        if ".." in f[0]:
            a, b = (int(x, 16) for x in f[0].split(".."))
        else:
            a = b = int(f[0], 16)
        st = CODE[f[1]]
        mp = f[2] if len(f) > 2 and f[2] else None
        if mp is not None or st in "mx":
            # a mapping applies per code point
            for cp in range(a, b + 1):
                entries.append((cp, st, mp))
        else:
            entries.append((a, st, None))
    entries.sort()
    out = ["# UTS #46 IDNA mapping table, Unicode 17.0.0 (tools/gen_intl_uts46.py)"]
    last = None
    for cp, st, mp in entries:
        key = (st, mp)
        if mp is None and last == key:
            continue
        last = key
        out.append("%x %s%s" % (cp, st, (" " + mp.lower()) if mp else ""))
    sys.stdout.write("\n".join(out) + "\n")


main()
