#!/usr/bin/env python3
"""Random pattern/subject pairs for differential testing of goro-regex
against php85. Usage: gen_random.py <seed> <count> > cases.txt
Output lines: hex(php pattern) TAB hex(subject)."""
import random
import sys

seed = int(sys.argv[1])
count = int(sys.argv[2])
R = random.Random(seed)

ALPHA = ["a", "b", "c", "A", "B", "-", " ", "\n", "1", "2"]
UALPHA = ALPHA + ["é", "É", "ß", "K", "k", "σ", "Σ"]


class G:
    def __init__(self, utf):
        self.ngroups = 0
        self.utf = utf
        self.depth = 0

    def lit(self):
        c = R.choice(UALPHA if self.utf else ALPHA)
        if c in "\n":
            return "\\n"
        if c == " ":
            return R.choice([" ", "\\x20", "[ ]"])
        if c == "-":
            return "\\-"
        return c

    def cls(self):
        items = []
        for _ in range(R.randint(1, 3)):
            k = R.random()
            if k < 0.4:
                items.append(self.lit().replace("[ ]", " "))
            elif k < 0.6:
                items.append("a-c")
            elif k < 0.8:
                items.append(R.choice(["\\d", "\\w", "\\s", "\\W", "[:alpha:]", "[:upper:]", "[:punct:]", "\\h"]))
            else:
                items.append(R.choice(["A-Z", "0-9", "\\n", "k", "\\p{Lu}", "\\p{L}"]))
        neg = "^" if R.random() < 0.3 else ""
        return "[" + neg + "".join(items) + "]"

    def atom(self):
        self.depth += 1
        try:
            k = R.random()
            if self.depth > 4:
                k = k * 0.5
            if k < 0.35:
                return self.lit()
            if k < 0.42:
                return "."
            if k < 0.52:
                return self.cls()
            if k < 0.58:
                return R.choice(["\\d", "\\w", "\\s", "\\b", "\\B", "^", "$", "\\A", "\\z", "\\Z", "\\R", "\\h", "\\v", "\\N"])
            if k < 0.75:
                kind = R.random()
                body = self.alt()
                if kind < 0.45:
                    self.ngroups += 1
                    return "(" + body + ")"
                if kind < 0.55:
                    self.ngroups += 1
                    return "(?<n%d>%s)" % (self.ngroups, body)
                if kind < 0.7:
                    return "(?:" + body + ")"
                if kind < 0.8:
                    return "(?>" + body + ")"
                if kind < 0.85:
                    return "(?i:" + body + ")"
                return "(?|" + body + ")"
            if k < 0.82:
                k2 = R.random()
                body = self.fixed()
                return R.choice(["(?=", "(?!", "(?<=", "(?<!"]) + body + ")"
            if k < 0.88 and self.ngroups > 0:
                g = R.randint(1, self.ngroups)
                return R.choice(["\\%d" % g, "\\g{%d}" % g, "\\g{-1}", "(?%d)" % g if R.random() < 0.3 else "\\%d" % g])
            if k < 0.91 and self.ngroups > 0:
                return "(?(%d)%s|%s)" % (R.randint(1, self.ngroups), self.seq(), self.seq())
            if k < 0.93:
                return R.choice(["(*COMMIT)", "(*PRUNE)", "(*SKIP)", "(*THEN)", "(*MARK:m)", "(*F)", "(*ACCEPT)", "\\K"])
            if k < 0.95:
                return "(?(?=%s)%s|%s)" % (self.lit(), self.seq(), self.seq())
            return self.lit()
        finally:
            self.depth -= 1

    def fixed(self):
        # A lookbehind-safe body.
        parts = []
        for _ in range(R.randint(1, 3)):
            k = R.random()
            if k < 0.6:
                parts.append(self.lit())
            elif k < 0.8:
                parts.append(self.cls())
            else:
                parts.append(".")
        s = "".join(parts)
        if R.random() < 0.3:
            s += "|" + self.lit()
        return s

    def quant(self, a):
        k = R.random()
        if k < 0.6 or a in ("^", "$", "\\b", "\\B", "\\A", "\\z", "\\Z", "\\K") or a.startswith("(*"):
            return a
        q = R.choice(["*", "+", "?", "{2}", "{1,3}", "{0,2}", "{2,}", "{,2}"])
        q += R.choice(["", "", "?", "+"])
        return a + q

    def seq(self):
        return "".join(self.quant(self.atom()) for _ in range(R.randint(0, 4)))

    def alt(self):
        n = 1 if R.random() < 0.6 else R.randint(2, 3)
        return "|".join(self.seq() for _ in range(n))


def subject(utf):
    al = UALPHA if utf else ALPHA
    return "".join(R.choice(al) for _ in range(R.randint(0, 14)))


out = sys.stdout.buffer
for _ in range(count):
    utf = R.random() < 0.3
    g = G(utf)
    pat = g.alt()
    mods = ""
    for m in "imsxUu" if utf else "imsxUD":
        if R.random() < 0.15:
            mods += m
    if utf and "u" not in mods:
        mods += "u"
    p = "/" + pat.replace("/", "\\/") + "/" + mods
    for _ in range(3):
        s = subject(utf)
        out.write(p.encode("utf-8").hex().encode() + b"\t" + s.encode("utf-8").hex().encode() + b"\n")
