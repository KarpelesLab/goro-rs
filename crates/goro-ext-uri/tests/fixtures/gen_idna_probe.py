#!/usr/bin/env python3
"""Generates idna_probe.txt: for every code point >= U+0080 (no surrogates),
a host "a<cp>b" as UTF-8 and its Punycode A-label form "xn--...".
Line: <hex cp>\t<hex utf8 host>\t<hex xn-- host>"""
import sys
out = open(sys.argv[1], 'w')
for cp in range(0x80, 0x110000):
    if 0xD800 <= cp <= 0xDFFF:
        continue
    s = 'a' + chr(cp) + 'b'
    u = s.encode('utf-8').hex()
    a = ('xn--' + s.encode('punycode').decode('ascii')).encode().hex()
    out.write(f"{cp:x}\t{u}\t{a}\n")
