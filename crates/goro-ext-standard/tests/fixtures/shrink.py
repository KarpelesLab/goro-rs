#!/usr/bin/env python3
"""Shrinks a fixture driver's output for committing: results longer than
LIMIT characters are replaced by m<md5 of the encoded result>, which the
reader (tests/common/mod.rs) compares against the md5 of its own encoding.

Usage: shrink.py FULL_OUTPUT > fixture.txt
"""
import hashlib
import sys

LIMIT = 300

for line in open(sys.argv[1], encoding='latin-1'):
    line = line.rstrip('\n')
    if line.startswith('#') or ' = ' not in line:
        print(line)
        continue
    head, tail = line.split(' = ', 1)
    toks = tail.split(' ')
    if len(toks[0]) > LIMIT:
        toks[0] = 'm' + hashlib.md5(toks[0].encode('latin-1')).hexdigest()
    print(head + ' = ' + ' '.join(toks))
