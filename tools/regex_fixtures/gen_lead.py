#!/usr/bin/env python3
"""Random cases aimed at the one-pass fast paths: a leading repeat, optional
repeats, a literal, lookaheads and captures, on subjects built from the same
characters. Usage: gen_lead.py seed count > cases.txt (hex pattern TAB hex
subject per line, for gen.php)."""
import random
import sys

seed, count = int(sys.argv[1]), int(sys.argv[2])
rnd = random.Random(seed)
ITEMS = [r"\w", r"\d", r"\s", "[a-c]", "[^=]", "a", "b", " ", "[ab]", r"\D", ".", "[a-z]"]
LITS = ["=", "ab", "a", "x", "ing", "b=", " ", ":", "c"]
QUANT = ["+", "*", "?", "{2,}", "{1,3}", "+?", "*?", "++", "*+", ""]
MODS = ["", "", "i", "m", "s", "x", "u", "U"]


def piece():
    r = rnd.random()
    if r < 0.45:
        return rnd.choice(ITEMS) + rnd.choice(QUANT)
    if r < 0.7:
        return rnd.choice(LITS)
    if r < 0.8:
        return "(?=" + rnd.choice(LITS) + rnd.choice(["", r"\b", rnd.choice(ITEMS)]) + ")"
    if r < 0.88:
        return "(" + piece() + ")"
    if r < 0.94:
        return rnd.choice([r"\b", "^", "$", r"\B"])
    return "(?:" + piece() + "|" + piece() + ")"


for _ in range(count):
    pat = "".join(piece() for _ in range(rnd.randint(1, 5)))
    pat = "/" + pat.replace("/", r"\/") + "/" + rnd.choice(MODS)
    alphabet = "aabbc  =:x\nzing_1"
    subj = "".join(rnd.choice(alphabet) for _ in range(rnd.randint(0, 30)))
    print(pat.encode().hex() + "\t" + subj.encode().hex())
