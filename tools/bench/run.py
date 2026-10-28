#!/usr/bin/env python3
"""Runs tools/bench/*.php with each given PHP binary (best of N runs) and
prints the wall times and the ratio to the first binary.

usage: tools/bench/run.py [-n RUNS] BINARY [BINARY ...]
Extra php options can follow a binary after a comma:
  run.py target/release/goro "/path/php85,-n"
"""
import glob
import os
import subprocess
import sys
import time


def main():
    args = sys.argv[1:]
    runs = 3
    if len(args) >= 2 and args[0] == "-n":
        runs = int(args[1])
        args = args[2:]
    if not args:
        print(__doc__)
        return 1
    bins = [a.split(",") for a in args]
    here = os.path.dirname(os.path.abspath(__file__))
    files = sorted(glob.glob(os.path.join(here, "*.php")))
    print("%-14s" % "bench" + "".join("%12s" % os.path.basename(b[0])[:11] for b in bins) + "   ratio")
    for f in files:
        times = []
        for b in bins:
            best = None
            for _ in range(runs):
                t = time.perf_counter()
                subprocess.run(b + [f], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                d = time.perf_counter() - t
                best = d if best is None else min(best, d)
            times.append(best)
        ratio = "".join(" %6.2fx" % (times[0] / t) for t in times[1:])
        print("%-14s" % os.path.basename(f)[:-4] + "".join("%11.3fs" % t for t in times) + ratio)
    return 0


if __name__ == "__main__":
    sys.exit(main())
