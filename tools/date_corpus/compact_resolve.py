#!/usr/bin/env python3
"""Turn the corpus TSV files (canon.py output) into the compact resolve
fixture: hex(input), strtotime() on the two bases, then the three modify()
results as "U.u T e" or "E position message" (positions and messages of
the first parse error, without the repeated input).

Usage: compact_resolve.py OUT.tsv IN.tsv...
"""
import re
import sys

out = open(sys.argv[1], "w")
for path in sys.argv[2:]:
    for line in open(path, encoding="utf-8", errors="surrogateescape"):
        cols = line.rstrip("\n").split("\t")
        if len(cols) < 8 or cols[2:] == ["F"] * 6:
            continue
        res = [cols[0], cols[2], cols[3]]
        for m in cols[4:7]:
            if m.startswith("X:"):
                msg = m.split(" :: ")[1]
                g = re.search(r"\) at position (\d+) \((.*?)\): (.*)$", msg, re.S)
                res.append("E %s %s" % (g.group(1), g.group(3)) if g else "E ?")
            else:
                # Y-m-d H:i:s.u T e U
                parts = m.split(" ")
                us = parts[1].split(".")[1]
                res.append("%s.%s %s %s" % (parts[-1], us, parts[2], parts[3]))
        out.write("\t".join(res) + "\n")
