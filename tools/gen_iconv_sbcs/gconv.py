#!/usr/bin/env python3
"""Parse glibc gconv-modules: print module charsets (names.txt) and aliases (aliases.json)."""
import glob, json, sys
files = ['/usr/lib64/gconv/gconv-modules'] + sorted(glob.glob('/usr/lib64/gconv/gconv-modules.d/*.conf'))
mods = []
aliases = {}
for f in files:
    for line in open(f):
        p = line.split()
        if not p or p[0].startswith('#'):
            continue
        if p[0] == 'alias':
            aliases[p[1].rstrip('/')] = p[2].rstrip('/')
        elif p[0] == 'module' and p[2] == 'INTERNAL':
            n = p[1].rstrip('/')
            if n not in mods:
                mods.append(n)
out = sys.argv[1]
open(out + '/names.txt', 'w').write('\n'.join(mods) + '\n')
json.dump(aliases, open(out + '/aliases.json', 'w'), indent=0)
print(len(mods), len(aliases))
