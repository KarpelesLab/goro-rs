import re, sys
# Builds the committed fixture from the PHP 8.5 runs (run 2 and run 3 of
# driver.php / driver3.php): within each (zone, format) group, at most
# KEEP lines per behaviour signature (the message codes reported, whether
# the create succeeded, the zone kind and relative part), and never PHP's
# own 64-bit overflow cases (year -2^63 wraps around in PHP).
KEEP = int(sys.argv[2])
out, count = [], {}
for path in sys.argv[3:]:
    for line in open(path, encoding='latin-1'):
        c = line.rstrip('\n').split('\t')
        if len(c) < 5 or line.startswith('#'):
            continue
        if '9223372036854775808' in c[2] and c[1] in ('!X', '!x'):
            continue
        dp = c[3].split('|')
        codes = tuple(sorted(set(re.findall(r'=([^;#|]*)', dp[7]))))
        sig = (c[0], c[1], codes, c[4][:1], dp[8].split(',')[0] if dp[8] else '', dp[9] != '')
        n = count.get(sig, 0)
        if n >= (1 if c[0] in ('Pacific/Kiritimati', 'America/New_York') and len(c) > 5 else KEEP):
            continue
        count[sig] = n + 1
        out.append(line if line.endswith('\n') else line + '\n')
seen, res = set(), []
for l in out:
    if l not in seen:
        seen.add(l)
        res.append(l)
open(sys.argv[1], 'w', encoding='latin-1').write(''.join(res))
print(len(res), sum(len(l) for l in res))
