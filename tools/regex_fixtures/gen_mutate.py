#!/usr/bin/env python3
"""Mutated patterns (mostly invalid) for comparing compile errors and
offsets with php85. Usage: gen_mutate.py <seed> <count> < cases.txt > out
Reads hex(pattern) TAB hex(subject) lines and writes mutated variants."""
import random
import sys

R = random.Random(int(sys.argv[1]))
count = int(sys.argv[2])
INS = list("()[]{}\\*+?|^$.-:<>=!#&'\"PpkgxoNcRKQEC0123456789,_aAz") + [
    "(?", "(*", "\\x{", "\\p{", "\\g{", "\\k<", "(?<", "(?(", "[:", ":]", "{1,", "(?P", "\\o{", "\\N{", "(?#",
]
src = [l.rstrip("\n").split("\t") for l in sys.stdin if l.strip()]
out = sys.stdout
for _ in range(count):
    p, s = R.choice(src)
    pat = bytes.fromhex(p).decode("utf-8", "replace")
    # strip delimiters/modifiers: patterns look like /.../mods
    end = pat.rfind("/")
    body, mods = pat[1:end], pat[end + 1:]
    for _ in range(R.randint(1, 3)):
        k = R.random()
        pos = R.randint(0, len(body))
        if k < 0.4 and body:
            pos = min(pos, len(body) - 1)
            body = body[:pos] + body[pos + 1:]
        else:
            body = body[:pos] + R.choice(INS) + body[pos:]
    body = body.replace("/", "\\/")
    if body.endswith("\\") and not body.endswith("\\\\"):
        body += "\\"
    np = "/" + body + "/" + mods
    out.write(np.encode("utf-8").hex() + "\t" + s + "\n")
