"""Public base tables for the iconv charsets.

Shared by iconv_mkdata.py (which diffs them against what PHP 8.5 with glibc
2.43 iconv does) and gen_iconv.py (which applies the recorded differences and
writes the Rust tables). Every base comes from a public data file:

- Unicode MAPPINGS (https://unicode.org/Public/MAPPINGS/): ISO8859/*.TXT,
  VENDORS/MICSFT/{WINDOWS,PC,EBCDIC}/*.TXT, VENDORS/MISC/*.TXT,
  VENDORS/APPLE/*.TXT, OBSOLETE/EASTASIA/{JIS,KSC,OTHER}/*.TXT
- WHATWG Encoding Standard indexes (https://encoding.spec.whatwg.org/):
  index-jis0208.txt, index-jis0212.txt, index-euc-kr.txt, index-big5.txt,
  index-gb18030.txt, index-gb18030-ranges.txt

A base table is a dict {bytes: tuple of code points} holding the valid
sequences only.
"""
import os

DATA = os.environ.get('ICONV_DATA', '/tmp/claude-1000/goro-work/charset-iconv/data')


def _path(name):
    return os.path.join(DATA, name)


def unicode_file(name, col=1, keyfn=None):
    """A Unicode MAPPINGS file: "0xBYTES<tab>0xUNICODE ..." lines."""
    m = {}
    with open(_path(name), encoding='latin-1') as f:
        for line in f:
            p = line.split('#')[0].split()
            if len(p) <= col or not p[0].startswith('0x') or not p[col].startswith('0x'):
                continue
            code = int(p[0], 16)
            if keyfn:
                key = keyfn(code)
            else:
                key = code.to_bytes(max(1, (code.bit_length() + 7) // 8), 'big')
            if key is not None:
                m[key] = (int(p[col], 16),)
    return m


def whatwg_index(name):
    m = {}
    with open(_path(name), encoding='utf-8') as f:
        for line in f:
            p = line.split('#')[0].split()
            if len(p) >= 2:
                m[int(p[0])] = int(p[1], 16)
    return m


def ascii_part():
    return {bytes([i]): (i,) for i in range(0x80)}


def with_controls(m):
    """Unicode's vendor files sometimes leave out C0 controls and DEL."""
    for i in list(range(0x20)) + [0x7F]:
        m.setdefault(bytes([i]), (i,))
    return m


def euc_jp():
    m = ascii_part()
    for p, cp in whatwg_index('index-jis0208.txt').items():
        if p < 94 * 94:
            m[bytes([0xA1 + p // 94, 0xA1 + p % 94])] = (cp,)
    for p, cp in whatwg_index('index-jis0212.txt').items():
        m[bytes([0x8F, 0xA1 + p // 94, 0xA1 + p % 94])] = (cp,)
    for i in range(0xA1, 0xE0):
        m[bytes([0x8E, i])] = (0xFF61 + i - 0xA1,)
    return m


def cp932():
    """CP932.TXT plus the user-defined area F040-F9FC -> U+E000-U+E757 (WHATWG
    Encoding: Shift_JIS decoder, pointers 8836-10715)."""
    m = unicode_file('CP932.TXT')
    for p in range(8836, 10716):
        lead, t = p // 188, p % 188
        lead += 0x81 if lead < 0x1F else 0xC1
        m[bytes([lead, t + (0x40 if t < 0x3F else 0x41)])] = (0xE000 + p - 8836,)
    return m


def euc_kr_whatwg(euc_only):
    m = ascii_part()
    for p, cp in whatwg_index('index-euc-kr.txt').items():
        lead, trail = 0x81 + p // 190, 0x41 + p % 190
        if euc_only and (lead < 0xA1 or trail < 0xA1):
            continue
        m[bytes([lead, trail])] = (cp,)
    return m


def big5_whatwg():
    m = ascii_part()
    for p, cp in whatwg_index('index-big5.txt').items():
        t = p % 157
        m[bytes([0x81 + p // 157, t + (0x40 if t < 0x3F else 0x62)])] = (cp,)
    return m


def gb18030_two_byte():
    m = ascii_part()
    m[b'\x80'] = (0x80,)
    for p, cp in whatwg_index('index-gb18030.txt').items():
        t = p % 190
        m[bytes([0x81 + p // 190, t + (0x40 if t < 0x3F else 0x41)])] = (cp,)
    return m


def gb18030_ranges():
    """[(linear four-byte index, first code point)] for the BMP part."""
    return sorted(whatwg_index('index-gb18030-ranges.txt').items())


def gb2312():
    m = {k: v for k, v in unicode_file('CP936.TXT').items()
         if len(k) == 1 or (k[0] >= 0xA1 and k[1] >= 0xA1)}
    return m


def euc_tw():
    m = ascii_part()

    def key(code):
        plane, row, cell = code >> 16, (code >> 8) & 0xFF, code & 0xFF
        if plane == 1:
            return bytes([row | 0x80, cell | 0x80])
        return bytes([0x8E, 0xA0 + plane, row | 0x80, cell | 0x80])
    m.update(unicode_file('EA-CNS11643.TXT', 1, key))
    return m


# Charset id -> function returning its base table (None: no public table;
# the data file then holds the full observed table).
BASES = {
    'ISO-8859-%d' % i: (lambda i=i: unicode_file('8859-%d.TXT' % i))
    for i in (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14, 15, 16)
}
for _cp in (874, 1250, 1251, 1252, 1253, 1254, 1255, 1256, 1257, 1258):
    BASES['CP%d' % _cp] = (lambda c=_cp: with_controls(unicode_file('CP%d.TXT' % c)))
BASES['CP737'] = lambda: with_controls(unicode_file('CP737.TXT'))
for _cp in (437, 775, 850, 852, 855, 857, 860, 861, 862, 863, 864, 865, 866, 869):
    BASES['IBM%d' % _cp] = (lambda c=_cp: with_controls(unicode_file('CP%d.TXT' % c)))
for _cp in ('037', '500', '1026', '875'):
    BASES['IBM%s' % _cp] = (lambda c=_cp: unicode_file('CP%s.TXT' % c))
BASES.update({
    'KOI8-R': lambda: unicode_file('MISC-KOI8-R.TXT'),
    'KOI8-U': lambda: unicode_file('MISC-KOI8-U.TXT'),
    'IBM424': lambda: unicode_file('MISC-CP424.TXT'),
    'IBM856': lambda: unicode_file('MISC-CP856.TXT'),
    'RK1048': lambda: with_controls(unicode_file('MISC-KZ1048.TXT')),
    'MACINTOSH': lambda: with_controls(unicode_file('APPLE-ROMAN.TXT')),
    'MAC-CYRILLIC': lambda: with_controls(unicode_file('APPLE-CYRILLIC.TXT')),
    'MAC-IS': lambda: with_controls(unicode_file('APPLE-ICELAND.TXT')),
    'MAC-CENTRALEUROPE': lambda: with_controls(unicode_file('APPLE-CENTEURO.TXT')),
    'EUC-JP': euc_jp,
    'EUC-JP-MS': euc_jp,
    'SHIFT_JIS': lambda: with_controls(unicode_file('EA-SHIFTJIS.TXT')),
    'CP932': cp932,
    'IBM932': cp932,
    'IBM943': cp932,
    'EUC-KR': lambda: euc_kr_whatwg(True),
    'CP949': lambda: euc_kr_whatwg(False),
    'JOHAB': lambda: with_controls(unicode_file('EA-JOHAB.TXT')),
    'BIG5': lambda: unicode_file('CP950.TXT'),
    'BIG5-HKSCS': big5_whatwg,
    'GBK': lambda: unicode_file('CP936.TXT'),
    'GB2312': gb2312,
    'GB18030': gb18030_two_byte,
})
