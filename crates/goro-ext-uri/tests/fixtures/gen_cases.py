#!/usr/bin/env python3
"""Builds whatwg_cases.txt from the WPT url test data (BSD-3-Clause, web-platform-tests).

Each line: <tag>\t<hex input>\t<hex base or '-'>.  Strings are UTF-8 with
lone surrogates encoded as WTF-8 bytes (what a PHP string can carry).
"""
import json, sys

D = sys.argv[1]
out = open(sys.argv[2], 'w')

def h(s):
    if s is None:
        return '-'
    return s.encode('utf-8', 'surrogatepass').hex()

seen = set()
def emit(tag, inp, base):
    key = (inp, base)
    if key in seen:
        return
    seen.add(key)
    out.write(f"{tag}\t{h(inp)}\t{h(base)}\n")

for x in json.load(open(f"{D}/urltestdata.json")):
    if isinstance(x, dict):
        emit('wpt', x['input'], x.get('base'))
        # also without base when one is given and input could be absolute
for x in json.load(open(f"{D}/toascii.json")):
    if isinstance(x, dict):
        emit('toascii', 'https://' + x['input'] + '/x', None)
for x in json.load(open(f"{D}/IdnaTestV2.json")):
    if isinstance(x, dict):
        emit('idna', 'https://' + x['input'] + '/x', None)
