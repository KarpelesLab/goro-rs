#!/usr/bin/env python3
"""Turn the observations made on PHP 8.5 (glibc 2.43 iconv) into the
committed iconv data files under tools/charset/iconv-data/.

usage: python3 -I iconv_mkdata.py <obs-dir> <out-dir>

<obs-dir> holds the outputs of the observation drivers run on the reference
PHP binary (names.out from iconv_obs_names.php, tables.out from
iconv_obs_tables.php, translit.out from iconv_obs_translit.php, extra.out from
iconv_obs_extra.php). For each table charset this writes <id>.txt holding the
differences between the public base table (iconv_bases.py) and what PHP does,
plus the byte-sequence structure (which prefixes are incomplete, how many
bytes //IGNORE skips over an illegal sequence), which only observation can
give. Charsets without a public table get their whole observed table.

Every line of the data files is "observed on PHP 8.5 (glibc 2.43 iconv)".
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import iconv_bases  # noqa: E402

# Representative observed name -> (id, kind). Kinds: "table" (stateless byte
# table), "compose" (table plus composition of combining marks on decode),
# "builtin:<Variant>" (hand-written codec), "skip" (not implemented).
RENAME = {
    'US-ASCII': 'ASCII',
    'ISO_8859-1:1987': 'ISO-8859-1', 'ISO_8859-2:1987': 'ISO-8859-2',
    'ISO_8859-3:1988': 'ISO-8859-3', 'ISO_8859-4:1988': 'ISO-8859-4',
    'ISO_8859-5:1988': 'ISO-8859-5', 'ISO_8859-6:1987': 'ISO-8859-6',
    'ISO_8859-7:1987': 'ISO-8859-7', 'ISO_8859-8:1988': 'ISO-8859-8',
    'ISO_8859-9:1989': 'ISO-8859-9',
    'WINDOWS-874': 'CP874', 'WINDOWS-31J': 'CP932',
    'CP856': 'IBM856', 'CP875': 'IBM875',
    'STRK1048-2002': 'RK1048', 'MACCYRILLIC': 'MAC-CYRILLIC',
    'CP1282': 'MAC-CENTRALEUROPE', 'CP1361': 'JOHAB', 'CSUNICODE': 'UNICODE',
    'CSUCS4': 'UCS-4',
}
for _i in range(1250, 1259):
    RENAME['WINDOWS-%d' % _i] = 'CP%d' % _i
BUILTIN = {
    'ASCII': 'Ascii', 'UTF-8': 'Utf8', 'UTF-16': 'Utf16', 'UTF-16BE': 'Utf16Be',
    'UTF-16LE': 'Utf16Le', 'UTF-32': 'Utf32', 'UTF-32BE': 'Utf32Be',
    'UTF-32LE': 'Utf32Le', 'UCS-2': 'Ucs2', 'UCS-2BE': 'Ucs2Be',
    'UCS-4': 'Ucs4', 'UCS-4LE': 'Ucs4Le', 'UNICODE': 'Unicode', 'UTF-7': 'Utf7',
    'UTF-7-IMAP': 'Utf7Imap', 'ISO-2022-JP': 'Iso2022Jp',
    'ISO-2022-KR': 'Iso2022Kr',
    'GB18030': 'Gb18030',
}
COMPOSE = {'CP1255', 'CP1258', 'TCVN'}
SKIP = {'EUC-TW', 'ISO-2022-JP-2', 'ISO-2022-CN', 'ISO-2022-CN-EXT', 'ISO-2022-JP-3', 'TSCII', 'CP930',
        'CP933', 'CP935', 'CP937', 'CP939', 'CP1364', 'CP1371', 'CP1388',
        'CP1390', 'CP1399'}


def charset_id(rep):
    u = rep.upper()
    return RENAME.get(u, u)


def name_groups(obs_dir):
    """[(representative, [names])]: the accepted names of names.out grouped by
    their short fingerprint, split further by the full checksums of
    aliases.out (iconv_obs_aliases.php)."""
    full = {}
    path = os.path.join(obs_dir, 'aliases.out')
    if os.path.exists(path):
        with open(path, encoding='latin-1') as f:
            for line in f:
                p = line.split()
                full[p[1]] = (p[2], p[3])
    groups, order = {}, []
    with open(os.path.join(obs_dir, 'names.out'), encoding='latin-1') as f:
        for line in f:
            p = line.rstrip('\n').split('\t')
            if p[0] != 'ACCEPT':
                continue
            name = p[1]
            key = (p[3], full.get(name))
            if full and key[1] is None:
                # A spelling variant (suffix, spaces): file it with the first
                # group of its fingerprint.
                key = next((k for k in order if k[0] == p[3]), key)
            if key not in groups:
                groups[key] = []
                order.append(key)
            groups[key].append(name)
    return [(groups[k][0], groups[k]) for k in order]


def representatives(obs_dir):
    """{charset id: representative name}."""
    return {charset_id(rep): rep for rep, _ in name_groups(obs_dir)}


def parse_names(obs_dir):
    """[(id, [aliases])] in first-seen order."""
    out = []
    for rep, names in name_groups(obs_dir):
        cid = charset_id(rep)
        # Keep only plain alias spellings: the probe also tried variants with
        # suffixes, spaces and junk characters, which the name normaliser
        # handles.
        aliases = []
        for n in names:
            if '//' in n or n != n.strip() or ' ' in n:
                continue
            n = n.upper()
            if n.endswith('/') and not n.endswith('//'):
                continue
            if n not in aliases:
                aliases.append(n)
        assert cid in aliases, (cid, aliases)
        aliases.remove(cid)
        out.append((cid, aliases))
    return out


ENTRY = re.compile(r'^[0-9A-F]+(\+[0-9A-F]+)*$')


def dump_files(obs_dir):
    return sorted(os.path.join(obs_dir, f) for f in os.listdir(obs_dir)
                  if f.startswith('tables') and f.endswith('.out'))


def parse_dump(obs_dir):
    """{rep: [(prefix bytes, [256 entries])]} in file order, from tables*.out."""
    dumps = {}
    for path in dump_files(obs_dir):
        with open(path, encoding='latin-1') as f:
            for line in f:
                if not line.startswith('D '):
                    continue
                p = line.split()
                pre = b'' if p[2] == '-' else bytes.fromhex(p[2])
                dumps.setdefault(p[1], []).append((pre, p[3:]))
    return dumps


def parse_enc(obs_dir):
    enc = {}
    for path in dump_files(obs_dir):
        with open(path, encoding='latin-1') as f:
            for line in f:
                if line.startswith('E '):
                    _, cs, cp, b = line.split()
                    enc.setdefault(cs, {})[int(cp, 16)] = None if b == 'x' else bytes.fromhex(b)
    return enc


def decode_with(table, seq):
    """Decode seq with //IGNORE semantics using an observed structure table
    {seq: ('c', cps) | ('i',) | ('x', skip)}; None when it ends incomplete."""
    out, i = [], 0
    while i < len(seq):
        j = i + 1
        while True:
            e = table.get(seq[i:j])
            if e is None:
                return None
            if e[0] == 'c':
                out.extend(e[1])
                i = j
                break
            if e[0] == 'x':
                i += e[1]
                break
            if e[0] == 'r':
                out.append(0xFFFD)
                i += e[1]
                break
            if j == len(seq):
                return None
            j += 1
    return tuple(out)


def observed_table(lines):
    """From the dump lines: {seq: ('c', cps) | ('i',) | ('x', skip)}."""
    t = {}
    raw = {}
    for pre, entries in lines:
        for b, e in enumerate(entries):
            seq = pre + bytes([b])
            if ENTRY.match(e):
                t[seq] = ('c', tuple(int(x, 16) for x in e.split('+')))
            elif e == 'i':
                t[seq] = ('i',)
            elif e == 'e':
                t[seq] = ('c', ())
            else:
                raw[seq] = e
    # Resolve the //IGNORE skip counts, shortest sequences first so the
    # simulation of the remaining bytes can use them.
    for seq in sorted(raw, key=len):
        e = raw[seq]
        kept = () if e == 'x' else ('?',) if e in ('x:i', 'x:x') else \
            tuple(int(x, 16) for x in e[2:].split('+'))
        skip = None
        for k in range(1, len(seq) + 1):
            t[seq] = ('x', k)
            rest = decode_with(t, seq[k:]) if k < len(seq) else ()
            if kept == ('?',):
                if rest is None or k < len(seq):
                    skip = k
                    break
            elif rest == kept:
                skip = k
                break
        if skip is None and kept and kept[0] == 0xFFFD:
            # //IGNORE writes U+FFFD for it (EUC-JP-MS 8E E0-FF).
            for k in range(1, len(seq) + 1):
                rest = decode_with(t, seq[k:]) if k < len(seq) else ()
                if rest == kept[1:]:
                    t[seq] = ('r', k)
                    break
            if t[seq][0] == 'r':
                continue
        t[seq] = ('x', skip or len(seq))
    return t


def fmt_cps(cps):
    return '+'.join('%X' % c for c in cps) if cps else 'e'


def ranges(bs):
    bs = sorted(bs)
    out = []
    for b in bs:
        if out and out[-1][1] == b - 1:
            out[-1][1] = b
        else:
            out.append([b, b])
    return ','.join('%02x' % a if a == z else '%02x-%02x' % (a, z) for a, z in out)


def structure_from(valid, extra_incomplete=()):
    """The prefixes a base table implies are incomplete."""
    pre = set(extra_incomplete)
    for seq in valid:
        for k in range(1, len(seq)):
            pre.add(seq[:k])
    return pre


def natural_inverse(table):
    """cp -> bytes: shortest sequence first, then the lowest bytes."""
    inv = {}
    for seq in sorted((s for s, e in table.items() if e[0] == 'c' and len(e[1]) == 1),
                      key=lambda s: (len(s), s)):
        inv.setdefault(table[seq][1][0], seq)
    return inv


def write_table(out, cid, rep, lines, enc_lines, base, algo_enc=None, tail=()):
    base_name = 'none'
    if base == 'ascii':
        base, base_name = iconv_bases.ascii_part(), 'ascii'
    elif base:
        base_name = 'yes'
    obs = observed_table(lines)
    valid_obs = {s: e[1] for s, e in obs.items() if e[0] == 'c'}
    base = dict(base or {})
    d_lines = []
    for s in sorted(set(valid_obs) | set(base), key=lambda s: (len(s), s)):
        o, b = valid_obs.get(s), base.get(s)
        if o != b:
            d_lines.append('D %s %s' % (s.hex(), '-' if o is None else fmt_cps(o)))
    merged = dict(valid_obs)
    implied = structure_from(merged)
    incomplete = {s for s, e in obs.items() if e[0] == 'i'}
    i_lines = {}
    for s in sorted(incomplete - implied):
        i_lines.setdefault(s[:-1], []).append(s[-1])
    bad = implied - incomplete
    assert not bad, (cid, sorted(bad)[:5])
    s_lines = {}
    r_lines = {}
    for s, e in obs.items():
        if e[0] == 'x' and e[1] != 1:
            s_lines.setdefault(s[:-1], {}).setdefault(e[1], []).append(s[-1])
        if e[0] == 'r':
            r_lines.setdefault(s[:-1], {}).setdefault(e[1], []).append(s[-1])
    # Encoder: what the observation says each code point encodes to, versus
    # the natural inverse of the decode table.
    php_inv = {}
    for pre, entries in lines:
        for b, e in enumerate(entries):
            if ENTRY.match(e) and '+' not in e:
                php_inv.setdefault(int(e, 16), pre + bytes([b]))
    actual = dict(php_inv)
    actual.update(enc_lines)
    nat = natural_inverse(obs)
    e_lines = []
    for cp in sorted(set(actual) | set(nat)):
        a, n = actual.get(cp), nat.get(cp)
        if a != n:
            if n is None and algo_enc is not None and a == algo_enc(cp):
                continue
            e_lines.append('E %X %s' % (cp, 'x' if a is None else a.hex()))
    with open(os.path.join(out, cid + '.txt'), 'w') as f:
        f.write('# iconv charset %s: differences from the public base table, observed\n' % cid)
        f.write('# on PHP 8.5 (glibc 2.43 iconv) with tools/charset/iconv_obs_tables.php.\n')
        f.write('# D <bytes> <code points|e (no output)|- (illegal)>: decoding\n')
        f.write('# I <prefix> <bytes>: prefix+byte is incomplete (needs more input)\n')
        f.write('# S <prefix> <n> <bytes>: //IGNORE skips n bytes of prefix+byte (default 1)\n')
        f.write('# R <prefix> <n> <bytes>: illegal, but //IGNORE writes U+FFFD for the n bytes\n')
        f.write('# E <code point> <bytes|x>: encoding, when not the shortest/lowest sequence\n')
        f.write('base %s\n' % base_name)
        for l in d_lines:
            f.write(l + '\n')
        for p in sorted(i_lines):
            f.write('I %s %s\n' % (p.hex() or '-', ranges(i_lines[p])))
        for p in sorted(s_lines):
            for n in sorted(s_lines[p]):
                f.write('S %s %d %s\n' % (p.hex() or '-', n, ranges(s_lines[p][n])))
        for p in sorted(r_lines):
            for n in sorted(r_lines[p]):
                f.write('R %s %d %s\n' % (p.hex() or '-', n, ranges(r_lines[p][n])))
        for l in e_lines:
            f.write(l + '\n')
        for l in tail:
            f.write(l + '\n')
    return len(d_lines), len(e_lines)


UCD = None


def compositions(cid, lines, path):
    """Decode-side composition: UnicodeData.txt canonical pairs whose base
    and mark both decode alone, versus the observed pairs."""
    global UCD
    if UCD is None:
        import iconv_translit_base
        UCD = iconv_translit_base.load_ucd()
    decomp, cat = UCD
    single = {}
    for pre, entries in lines:
        if pre == b'':
            for b, e in enumerate(entries):
                if ENTRY.match(e) and '+' not in e:
                    single[b] = int(e, 16)
    obs = {}
    with open(path) as f:
        for line in f:
            p = line.split()
            if p[0] == 'C' and p[1] == cid:
                obs[(single[int(p[2], 16)], single[int(p[3], 16)])] = \
                    None if p[4] == 'x' or '+' in p[4] else int(p[4], 16)
    chars = set(single.values())
    base = {}
    for cp, (compat, parts) in decomp.items():
        if not compat and len(parts) == 2 and parts[0] in chars and parts[1] in chars \
                and cat.get(parts[1]) == 'Mn':
            base[parts] = cp
    out = ['# C <base> <mark> <composed|->: decoding composes base+mark (differences from',
           '# the canonical pairs of UnicodeData.txt)']
    for k in sorted(set(base) | set(obs)):
        if base.get(k) != obs.get(k):
            out.append('C %X %X %s' % (k[0], k[1], '-' if obs.get(k) is None else '%X' % obs[k]))
    return out


def gb18030_four_byte(path):
    """The observed four-byte map {linear index: cp} as runs, compared with
    WHATWG's index-gb18030-ranges (BMP) and the linear SMP mapping."""
    runs = []
    with open(path) as f:
        for line in f:
            p = line.split()
            if p[0] == 'G':
                runs.append((int(p[1]), int(p[2], 16), int(p[3])))
    obs = {}
    for idx, cp, n in runs:
        for k in range(n):
            obs[idx + k] = cp + k
    exp = {}
    rng = iconv_bases.gb18030_ranges()
    j = 0
    for idx in range(39420):
        while j + 1 < len(rng) and rng[j + 1][0] <= idx:
            j += 1
        exp[idx] = rng[j][1] + idx - rng[j][0]
    for cp in range(0x10000, 0x110000):
        exp[189000 + cp - 0x10000] = cp
    lines = []
    for idx in sorted(set(obs) | set(exp)):
        if obs.get(idx) != exp.get(idx):
            lines.append('G %d %s' % (idx, '-' if idx not in obs else '%X' % obs[idx]))
    inv = {cp: idx for idx, cp in obs.items()}

    def seq(idx):
        b4 = idx % 10
        idx //= 10
        b3 = idx % 126
        idx //= 126
        return bytes([0x81 + idx // 10, 0x30 + idx % 10, 0x81 + b3, 0x30 + b4])
    return lines, (lambda cp: seq(inv[cp]) if cp in inv else None)


def main():
    obs_dir, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    names = parse_names(obs_dir)
    dumps = parse_dump(obs_dir)
    encs = parse_enc(obs_dir)
    rep_of = representatives(obs_dir)
    with open(os.path.join(out, 'charsets.txt'), 'w') as f:
        f.write('# iconv charsets: <id> <kind> <aliases...>\n')
        f.write('# The aliases are the names PHP 8.5 (glibc 2.43 iconv) accepts, found by\n')
        f.write('# probing the IANA registry names and common spellings\n')
        f.write('# (tools/charset/iconv_names_candidates.py, iconv_obs_names.php).\n')
        for cid, aliases in names:
            if cid in BUILTIN:
                kind = 'builtin:' + BUILTIN[cid]
            elif cid in SKIP:
                kind = 'skip'
            elif cid in COMPOSE:
                kind = 'compose'
            else:
                kind = 'table'
            f.write('%s %s %s\n' % (cid, kind, ' '.join(aliases)))
            if kind in ('table', 'compose') or cid == 'GB18030':
                rep = rep_of[cid]
                base_fn = iconv_bases.BASES.get(cid)
                base = base_fn() if base_fn else None
                if base is None:
                    # No public table: the data file holds the whole observed
                    # table, written as differences from ASCII when that is
                    # shorter.
                    obs0 = {pre + bytes([b]): e for pre, es in dumps[rep] if pre == b''
                            for b, e in enumerate(es)}
                    same = sum(1 for i in range(128) if obs0.get(bytes([i])) == '%X' % i)
                    if same > 64:
                        base = 'ascii'
                algo, tail = None, ()
                if cid == 'GB18030':
                    tail, algo = gb18030_four_byte(os.path.join(obs_dir, 'extra.out'))
                    tail = ['# G <four-byte linear index> <code point|->: differences from WHATWG'
                            ' index-gb18030-ranges'] + tail
                if kind == 'compose':
                    tail = compositions(cid, dumps[rep], os.path.join(obs_dir, 'extra.out'))
                nd, ne = write_table(out, cid, rep, dumps[rep], encs.get(rep, {}), base, algo, tail)
                print('%-20s %-8s D %5d E %5d' % (cid, base if isinstance(base, str) else 'base' if base else 'observed', nd, ne))


if __name__ == '__main__':
    main()
