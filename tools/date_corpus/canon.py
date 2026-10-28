"""Convert parse_out.jsonl to a line format the Rust tests read:

hex(input) TAB canonical(date_parse) TAB t1 TAB t2 TAB m1 TAB m2 TAB m3 TAB c
"""
import json, sys


def canon(p):
    f = []
    for k in ['year', 'month', 'day', 'hour', 'minute', 'second']:
        v = p[k]
        f.append('F' if v is False else str(v))
    fr = p['fraction']
    f.append('F' if fr is False else str(round(fr * 1e6)))
    f.append(';'.join('%d:%s' % (a, b) for a, b in p['warnings']))
    f.append(';'.join('%d:%s' % (a, b) for a, b in p['errors']))
    f.append('1' if p['is_localtime'] else '0')
    f.append(str(p['zone_type']) if 'zone_type' in p else '-')
    f.append(str(p['zone']) if 'zone' in p else '-')
    f.append(('1' if p['is_dst'] else '0') if 'is_dst' in p else '-')
    f.append(p.get('tz_abbr', '-'))
    f.append(p.get('tz_id', '-'))
    if 'relative' in p:
        r = p['relative']
        parts = [str(r[k]) for k in ['year', 'month', 'day', 'hour', 'minute', 'second']]
        if 'weekday' in r:
            parts.append('wd=%d' % r['weekday'])
        if 'weekdays' in r:
            parts.append('wds=%d' % r['weekdays'])
        if r.get('first_day_of_month'):
            parts.append('fdm')
        if r.get('last_day_of_month'):
            parts.append('ldm')
        f.append(','.join(parts))
    else:
        f.append('-')
    return '|'.join(f)


def val(v):
    if v is False:
        return 'F'
    if isinstance(v, list):
        return 'X:' + ' :: '.join(str(x) for x in v)
    return str(v)


out = open(sys.argv[2], 'w')
for line in open(sys.argv[1]):
    r = json.loads(line)
    s = r['s'].encode('utf-8', 'surrogatepass')
    cols = [s.hex(), canon(r['p'])] + [val(r.get(k, 'F')) for k in ['t1', 't2', 'm1', 'm2', 'm3', 'c']]
    out.write('\t'.join(c.replace('\t', ' ').replace('\n', ' ') for c in cols) + '\n')
