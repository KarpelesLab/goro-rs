#!/usr/bin/env python3
"""Random (seeded) URI-like inputs for both parsers, in the formats of the
other case files: rfc3986 (P/R/W/E lines), whatwg (tag fuzz) and whatwg
setters."""
import random, sys

rnd = random.Random(int(sys.argv[3]) if len(sys.argv) > 3 else 20261028)
out_dir = sys.argv[1]
N = int(sys.argv[2]) if len(sys.argv) > 2 else 5000

schemes = ['http', 'https', 'HTTP', 'ws', 'wss', 'ftp', 'file', 'foo', 'a+b', 'sc', 'mailto', '1a', '', 'Fi\u0131e']
seps = [':', '://', ':/', ':\\\\', '//', '/', '', ':///', '://\\']
users = ['', '', '', 'u@', 'u:p@', ':p@', '@', '%41@', '\u00e9@', 'a@b@', 'u:p:q@', '[u]@']
hosts = ['h', 'H.COM', '1.2.3.4', '[::1]', '[v1.x]', 'xn--ls8h', '\u00e9.com', '%41', 'a%2eb', '256', '0x7f.1', '[1:2::3]', '',
         'a b', 'ex%00', '[::1.2.3.4]', 'localhost', 'C|', 'c:', '\uff21.com', 'xn--', 'a..b', '.', '1.2.3.4.5', '[::', 'h:x',
         '\u0661\u0662', 'a\u200db', 'XN--LS8H', 'www.example.com', '%e2%98%83', '09', '0x', '1.', '[0:0::0:0]']
ports = ['', '', '', ':', ':80', ':0443', ':99999', ':a', ':65535', ':0', ':21']
paths = ['', '/', '/a/b', '/a/../b', '/%2e%2E/', '/./x', '/a b', '/\u00e9', '/%7e', 'a:b', '/a?b', '\\x', '/C:/', '/|/',
         '/..', '/.', '//x', '/a/./b/../../c', '/%zz', '/\x00', '/a/..%2f', '/~user', '/a%2Fb', '/^', '/{}', 'x/y', '/a/b/']
queries = ['', '', '?', '?a=b', '?a b', '?%zz', '?\u00e9', "?'", '?a?b', '?#', '?\x01', '?%41']
frags = ['', '', '#', '#f', '#a b', '#\u00e9', '#`', '#a#b', '#%41', '#<>']
pool = list('azAZ09:/?#[]@!$&\'()*+,;=-._~% "<>\\^`{|}\t\n\r\x00\x01\x7f') + ['\u00e9', '\u00a0', '\u3002', '\uff0e', '\u00ad', '\ud800', '\xff']

def mutate(s):
    for _ in range(rnd.choice([0, 0, 1, 1, 2, 3])):
        op = rnd.random()
        i = rnd.randint(0, len(s))
        c = rnd.choice(pool)
        if op < 0.5:
            s = s[:i] + c + s[i:]
        elif op < 0.75 and s:
            i = min(i, len(s) - 1)
            s = s[:i] + s[i + 1:]
        elif s:
            i = min(i, len(s) - 1)
            s = s[:i] + c + s[i + 1:]
    return s

def gen():
    if rnd.random() < 0.1:
        return mutate(''.join(rnd.choice(pool) for _ in range(rnd.randint(0, 12))))
    s = rnd.choice(schemes) + rnd.choice(seps) + rnd.choice(users) + rnd.choice(hosts) + rnd.choice(ports) + rnd.choice(paths) + rnd.choice(queries) + rnd.choice(frags)
    if rnd.random() < 0.1:
        s = rnd.choice([' ', '\t', '\x01 ', '']) + s + rnd.choice([' ', '\n', ''])
    return mutate(s)

def enc(s):
    if isinstance(s, bytes):
        return s.hex()
    return s.replace('\xff', '\udcff').encode('utf-8', 'surrogatepass').replace(b'\xed\xb3\xbf', b'\xff').hex()

bases_w = [None, None, 'http://b/c/d?q#f', 'file:///C:/x/y', 'foo://h/a/b', 'sc:opaque', 'https://u:p@h:8/a/', 'file://host/share/x']
bases_r = ['http://a/b/c/d;p?q', 's:a/b', 's:/a/b', 'file:///x/y', 'HTTP://U@[::1]:8/%7e/']
wops_r = ['scheme', 'userinfo', 'host', 'port', 'path', 'query', 'fragment']
wops_w = ['withScheme', 'withUsername', 'withPassword', 'withHost', 'withPort', 'withPath', 'withQuery', 'withFragment']

with open(f'{out_dir}/rfc_fuzz_cases.txt', 'w') as rf, open(f'{out_dir}/whatwg_fuzz_cases.txt', 'w') as wf, open(f'{out_dir}/whatwg_setters_fuzz_cases.txt', 'w') as sf:
    for i in range(N):
        s = gen()
        r = rnd.random()
        if r < 0.55:
            rf.write(f'P\t{enc(s)}\n')
        elif r < 0.7:
            rf.write(f'R\t{enc(rnd.choice(bases_r))}\t{enc(s)}\n')
        elif r < 0.85:
            op = rnd.choice(wops_r)
            if op == 'port':
                a = rnd.choice(['N', 'I0', 'I80', 'I-1', 'I65536'])
                a = a.encode().hex()
            else:
                v = rnd.choice([None, gen(), rnd.choice(hosts), rnd.choice(paths), mutate(rnd.choice(users).rstrip('@'))])
                if v is None and op == 'path':
                    v = ''
                a = 'N'.encode().hex() if v is None else (b'S'.hex() + enc(v))
            rf.write(f'W\t{enc(rnd.choice(["http://h/p?q#f", "//h", "s:a:b", "foo", "", "HTTP://U@H:8/x"]))}\t{op.encode().hex()}\t{a}\n')
        else:
            rf.write(f'E\t{enc(s)}\t{enc(mutate(s))}\n')
        b = rnd.choice(bases_w)
        wf.write(f'fuzz\t{enc(s)}\t{"-" if b is None else enc(b)}\n')
        if i % 2 == 0:
            op = rnd.choice(wops_w)
            if op == 'withPort':
                a = rnd.choice(['N', 'I0', 'I80', 'I443', 'I-1', 'I65536', 'I8080'])
            else:
                v = rnd.choice([None, gen(), rnd.choice(hosts), rnd.choice(paths), rnd.choice(queries), rnd.choice(schemes), mutate('x')])
                if v is None and op in ('withScheme', 'withPath'):
                    v = ''
                a = 'N' if v is None else 'S' + enc(v)
            base = rnd.choice(['http://u:p@h:8/p?q#f', 'foo://h/p', 'file:///x', 'sc:op', 'https://example.com/', 'ws://w/', 'foo:///x', ' http://h/a b'])
            sf.write(f'{enc(base)}\t{op}\t{a}\n')
