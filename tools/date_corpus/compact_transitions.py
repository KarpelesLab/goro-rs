#!/usr/bin/env python3
"""Compact php85 getTransitions() dumps into a fixture of checksums.

Usage: compact_transitions.py OUT.txt ALL.txt TAIL.txt

ALL.txt is the output of a php85 script printing, for every identifier,
"Z name" then "ts offset isdst abbr" for getTransitions() (from
PHP_INT_MIN to the default end); TAIL.txt the same for
getTransitions(2147483647, 4300000000). The fixture keeps, per zone and
range, the entry count and a CRC-32 of the lines joined with "\\n".
"""
import sys
import zlib


def read(path):
    zones = {}
    cur = None
    for line in open(path):
        line = line.rstrip("\n")
        if line.startswith("Z "):
            cur = line[2:]
            zones[cur] = []
        else:
            zones[cur].append(line)
    return zones


def main():
    out = open(sys.argv[1], "w")
    for tag, path in (("all", sys.argv[2]), ("tail", sys.argv[3])):
        for name, lines in sorted(read(path).items()):
            crc = zlib.crc32("\n".join(lines).encode()) & 0xFFFFFFFF
            out.write("%s\t%s\t%d\t%08x\n" % (tag, name, len(lines), crc))


if __name__ == "__main__":
    main()
