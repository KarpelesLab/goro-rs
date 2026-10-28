#!/usr/bin/env python3
"""Generates rfc3986_cases.txt: probe inputs for Uri\\Rfc3986\\Uri (hand-written
and systematic), plus the WPT inputs.  Ops (tab separated, hex args):
  P <uri>                parse + all getters
  R <base> <ref>         resolve
  E <a> <b>              equals (both modes)
  W <uri> <op> <arg|N>   wither (op = scheme/userinfo/host/port/path/query/fragment)
"""
import json, sys

D = sys.argv[1]
out = open(sys.argv[2], 'w')

def h(s):
    if s is None:
        return 'N'
    if isinstance(s, str):
        s = s.encode('utf-8', 'surrogatepass')
    return s.hex()

seen = set()
def emit(*f):
    key = f
    if key in seen:
        return
    seen.add(key)
    out.write('\t'.join([f[0]] + [h(x) if not isinstance(x, int) else str(x) for x in f[1:]]) + '\n')

def P(s): emit('P', s)

# --- RFC 3986 examples and hand-written cases
hand = """
ftp://ftp.is.co.za/rfc/rfc1808.txt
http://www.ietf.org/rfc/rfc2396.txt
ldap://[2001:db8::7]/c=GB?objectClass?one
mailto:John.Doe@example.com
news:comp.infosystems.www.servers.unix
tel:+1-816-555-1212
telnet://192.0.2.16:80/
urn:oasis:names:specification:docbook:dtd:xml:4.1.2
http://example.com/~smith/
http://EXAMPLE.COM/%7Esmith/
HTTP://www.Example.com/
http://example.com:/
http://example.com:80/
http://a/b/c/./../../g
mid/content=5/../6
eXAMPLE://a/./b/../b/%63/%7bfoo%7d
http://[::1]
http://[::]
http://[1::]
http://[1::2]
http://[1:2:3:4:5:6:7:8]
http://[1:2:3:4:5:6:7::]
http://[::2:3:4:5:6:7:8]
http://[1:2:3:4:5:6:1.2.3.4]
http://[::1.2.3.4]
http://[::ffff:1.2.3.4]
http://[1:2:3:4:5:6:7:8:9]
http://[1:2:3:4:5:6:7]
http://[1::2::3]
http://[:::]
http://[1:::2]
http://[12345::]
http://[0001:2::]
http://[00001::]
http://[1:2:3:4:5:1.2.3.4]
http://[1:2:3:4:5:6:7:1.2.3.4]
http://[::1.2.3]
http://[::1.2.3.4.5]
http://[::256.1.1.1]
http://[::01.1.1.1]
http://[::1.1.1.01]
http://[::1.1.1.1:2]
http://[fe80::1%25eth0]
http://[fe80::1%eth0]
http://[v1.x]
http://[V1.x]
http://[vA.x:y]
http://[v1.]
http://[v.x]
http://[vg.x]
http://[v1x]
http://[v1.x/y]
http://[v1.~!$&'()*+,;=:]
http://[]
http://[
http://]
http://[::1]]
http://[::1]:80
http://[::1]x
http://a[b]
http://1.2.3.4
http://1.2.3.4.
http://1.2.3
http://01.2.3.4
http://256.2.3.4
http://1.2.3.4:65536
http://1.2.3.4:999999999999
http://h:9223372036854775807
http://h:9223372036854775808
http://h:018446744073709551616
http://h:0
http://h:00
http://h:-1
http://h:+1
http://h:1a
http://h:1:2
http://h::
http://:80
http://@:80
http://u:p:q@h
http://u@p@h
http://u%@h
http://u%4@h
http://u%41@h
http://%zz@h
http://h%zz
http://h/%zz
http://h/?%zz
http://h/#%zz
http://h/%
http://h/%4
http://h/%4g
http://h/%c3%a9
http://h/%C3%A9
http://h/%7e
http://h/%7E
http://h/%2e
http://h/%2E%2e/
http://h/a/%2e%2e/b
http://h/a/%2E/b
http://h/a/.%2e/b
http://h/.
http://h/..
http://h/./
http://h/../
http://h/a/b/..
http://h/a/b/.
http://h/a/b/../
http://h/a//b
http://h//a
http:///a
http:a
http:/a
http:
http://
//
///
////
/
//h
//h:1
//@
//u@
//:1
//[::1]
?
#
?#
#?
a?b#c
a#b?c
a#b#c
a?b?c
a:b
a:
1a:b
+a:b
a+b-c.d:e
a_b:c
a~b:c
a%62:c
:a
./a:b
a/b:c
.
..
./
../
./..
a/..
a/../..
a/./b/../c
/./a
/../a
/a/./b/../../../c
s:a/../../b
s:/a/../../b
s:a/./b
s://h/a/../../b
s://h
s://h?q
s://h#f
s://h/a?q#f
s://u:p@h:1/a?q#f
s://u:@h
s://:p@h
s://:@h
s://@h
s://u@h
s://[::1]:1
s://h:1?q
s://h?q/r?s
s://h#f/g?h#i
s://h/a@b:c
s://h/a[b
s://h/a]b
s://h?a[b]
s://h#a[b]
s://h/a{b
s://h/a b
s://h/a"b
s://h/a<b
s://h/a\\b
s://h/a^b
s://h/a`b
s://h/a|b
s://h/a!$&'()*+,;=:@-._~b
s://h?a!$&'()*+,;=:@-._~/?b
s://h#a!$&'()*+,;=:@-._~/?b
s://!$&'()*+,;=-._~@h
s://u!$&'()*+,;=:-._~@h
s://!$&'()*+,;=-._~
s://h!$&'()*+,;=-._~
S://H.EXAMPLE.COM/P?Q#F
S://%48.%45/%50?%51#%46
http://ExAmPlE.CoM/PaTh?QuErY#FrAgMeNt
http://%65%78%61%6d%70%6c%65.com
http://%C3%A9.com
http://%c3%a9.com
http://e%2Example.com
http://a%2Fb
http://a%3Ab
http://a%40b
http://a%5Bb
http://a%00b
http://h/a%00b
http://h/a%2fb
http://h/a%2Fb
http://h/%41%42%43
http://h/%61%62%63
http://h/%5F%2D%2E%7E
http://h/%21%24%26%27%28%29%2A%2B%2C%3B%3D
http://h/%3A%40%2F%3F%23%5B%5D
http://h/%20
http://h/?%20
http://h/#%20
http://u%3Ap@h
http://u:p%3Aq@h
http://%55:%50@h
http://U:P@H
http://[::A]
http://[::a]
http://[A::]
http://[1:0:0:0:0:0:0:1]
http://[1:0:0:1:0:0:0:1]
http://[0:0:0:0:0:0:0:0]
http://[0000:0000:0000:0000:0000:0000:0000:0000]
http://[::0.0.0.0]
http://[::FFFF:192.168.1.1]
http://[2001:DB8::1]
http://[2001:db8:0:0:1:0:0:1]
http://[2001:0db8::0001]
file:///etc/passwd
file://localhost/etc/passwd
file:/etc/passwd
file:etc/passwd
file:///C:/x
file:///c|/x
file://h/a/../b
mailto:a@b?subject=x
data:text/plain,hello
data:,a b
javascript:alert(1)
about:blank
urn:a:b
tag:a,2000:b
http://a/b/c/d;p?q
http://a/b/c/d;p?q#f
a:b:c
a:/b:c
a://b:c
a://b:c/d
a://b:/d
a://:/d
a://b:c:d
http://h:80a
http://h:80/a
http://h:80?a
http://h:80#a
http://u@h:80@x
http://[::1]@h
http://h@[::1]
http://h@[::1]:2
http://user:pass@host:123/path?query#fragment
https://user:pass@example.com:8080/path/to/file.php?query=1&a[]=2#frag
https://example.com/path/to/file.php?q=a%20b
https://example.com/a/b/c/../../d/./e?x#y
https://example.com:443/
https://example.com:0443/
https://www.example.com/index.html
http://127.0.0.1:8080/
http://localhost/
"""
for line in hand.strip().split('\n'):
    P(line)

# --- dot segments in every context
dpaths = ['.', '..', './', '../', './.', '../.', './..', '../..', '././', '././a', './a', './a/', 'a/./', 'a/.', 'a/..', 'a/../', 'a/../..', 'a/../../', 'a/b/./', 'a/b/.', './/a', 'a/.//b', 'a/..//b', './../a', '../../a', '.././a', 'a/../b/../..', 'a/../.', 'a/.././', 'a//..', 'a//../b', '.a', '..a', 'a.', 'a..', '...', '.../a', './a:b', 'a/../b:c', 'x/..//a', './/', './a:b/..', './a:b/../..', './a:b/./..', '%2e', '%2e/', '%2e%2e/a', 'a/%2e%2e', '.%2E', '.%2e/', '%2e./', 'a/b/c/../../..', 'a/b/c/../../../..', '../a/../b', 'a/../../b/../c', '././..', '.././..', 'a/./../b', 'a:b', 'a/b:c/..', '%2E/a:b', 'a/%2e/b', 'a/b/%2E%2E/c']
for c in ['', 's:', 's:/', '//h/', 'http://h/', '/', 'S://U@H:1/']:
    for p in dpaths:
        P(c + p)

# --- Systematic: every byte at several positions of a template
templates = [
    ('scheme', 'a{}b://h/'),
    ('scheme1', '{}a://h/'),
    ('userinfo', 's://u{}v@h/'),
    ('host', 's://h{}i/'),
    ('host1', 's://{}h/'),
    ('port', 's://h:1{}/'),
    ('path', 's://h/p{}q'),
    ('relpath', 'p{}q'),
    ('relpath1', '{}q'),
    ('query', 's://h/?q{}r'),
    ('fragment', 's://h/#f{}g'),
    ('ipv6', 's://[::1{}]/'),
    ('ipvf', 's://[v1.a{}]/'),
]
for name, t in templates:
    for b in range(256):
        P(t.encode().replace(b'{}', bytes([b])))
    for mb in ['é', '🐘', '\u00a0', '\u3000']:
        P(t.replace('{}', mb))
    for pct in ['%', '%4', '%41', '%2F', '%2f', '%3a', '%7E', '%7e', '%25', '%00', '%zz', '%C3%A9', '%c3%a9', '%e2%82', '%FF']:
        P(t.replace('{}', pct))

# --- WPT inputs (most are not RFC 3986 references, which is the point)
for x in json.load(open(f"{D}/urltestdata.json")):
    if isinstance(x, dict):
        P(x['input'])
        if x.get('base'):
            P(x['base'])

# --- Resolution: RFC 3986 section 5.4 plus extras
base = 'http://a/b/c/d;p?q'
refs = """g:h
g
./g
g/
/g
//g
?y
g?y
#s
g#s
g?y#s
;x
g;x
g;x?y#s

.
./
..
../
../g
../..
../../
../../g
../../../g
../../../../g
/./g
/../g
g.
.g
g..
..g
./../g
./g/.
g/./h
g/../h
g;x=1/./y
g;x=1/../y
g?y/./x
g?y/../x
g#s/./x
g#s/../x
http:g
http:
http://
//
///
?
#
/
a:
mailto:x
//h/../a
//h/./a/../b
?a/../b
#a/../b
%2e%2e/g
.%2e/g
%2E/g
g/%2e%2e/h""".split('\n')
bases = ['http://[::1]/a/b', 'http://h:080/a', 'HTTP://U@H/%7e/a', base, 'http://a/b/c/d;p', 'http://a', 'http://a/', 'http://u:p@a:1/b/c?q#f', 'http://a/b/c/', 's:a/b', 's:/a/b', 's:', 's://h', 'file:///x/y', 'http://A/B/%7e/../C', 'HTTP://a/b/c', 'http://a/b/c/../d']
for b in bases:
    for r in refs:
        emit('R', b, r)
for b in ['s:/a/b', 's:/', 's:/a', 's:a', 's:a/', 'http://h', 'http://h/a/', '//h/a/b']:
    for r in ['/a/..', '/a/../', '/./', '/.', '/..', '/../', 'x/..', 'x/../', './', '.', '..', '../', './/', 'x/..//y', '..//', './/y', 'a:b', './a:b', '../a:b', '../../', '../..', '../../..', '../../../', 'x/../..', '/x/../..', '/../x', '.././', './../', '%2e/', 'x/%2e%2e/', '/.//x', 'x/.//y']:
        emit('R', b, r)
for r in ['http://x/y', 'a', '/a', '', 'a b', '%zz', 'é']:
    emit('R', 'a/b', r)
    emit('R', '/a/b', r)
    emit('R', '//h/a', r)

# --- equals
pairs = [
    ('http://a', 'http://a'), ('http://a', 'http://A'), ('HTTP://a', 'http://a'),
    ('http://a/', 'http://a'), ('http://a:80', 'http://a'), ('http://a:080', 'http://a:80'),
    ('http://a/%7e', 'http://a/~'), ('http://a/%7E', 'http://a/%7e'), ('http://a/%2f', 'http://a/%2F'),
    ('http://a/%41', 'http://a/a'), ('http://a/%41', 'http://a/A'), ('http://a/b/../c', 'http://a/c'),
    ('http://a/./c', 'http://a/c'), ('http://a?%41', 'http://a?A'), ('http://a#%41', 'http://a#A'),
    ('http://a#x', 'http://a#y'), ('http://a#x', 'http://a'), ('http://a#', 'http://a'),
    ('http://a?', 'http://a'), ('http://a@b', 'http://b'), ('http://@b', 'http://b'),
    ('http://A@b', 'http://a@b'), ('http://%41@b', 'http://A@b'), ('http://[::1]', 'http://[0::1]'),
    ('http://[::1]', 'http://[0:0:0:0:0:0:0:1]'), ('http://[::ffff:1.2.3.4]', 'http://[::ffff:102:304]'),
    ('http://[v1.a]', 'http://[V1.A]'), ('http://1.2.3.4', 'http://01.2.3.4'),
    ('a/b', 'a/b'), ('a/../b', 'b'), ('', ''), ('/', ''), ('//a', '//A'), ('s:a', 'S:a'),
    ('http://%65.com', 'http://e.com'), ('http://%C3%A9', 'http://%c3%a9'),
    ('http://a/%c3%a9', 'http://a/%C3%A9'), ('http://a:', 'http://a'), ('http://a:0', 'http://a:00'),
    ('mailto:A@B', 'mailto:a@b'), ('x://a/b', 'x://a/b/'),
]
for a, b in pairs:
    emit('E', a, b)

# --- withers
wuris = ['http://h:80', 'http://[::1]:80/a', 'http://u@h', 'http://h:/p', 'HTTP://U%41@[V1.A]:080/%7e?%41#%42', 'https://example.com', 'https://u:p@example.com:8080/p/q?q=1#f', '/foo/bar', 'foo', '', 's:a:b',
         '//h//p', 'mailto:a@b', 'file:///x', '?q', '#f', 'http://[::1]/', 'x:/a', 'x:', '//h', 'a/b:c']
wargs = {
    'scheme': [None, '', 'http', 'HTTP', 'h1', '1h', 'a+b-c.d', 'a_b', 'http:', 'http%73', 'é', 'a b', 's\0'],
    'userinfo': [None, '', 'u', 'u:p', ':p', 'u:', 'u:p:q', 'u@p', 'u/p', 'u%41', 'u%zz', 'é', '[u]', 'u!$&\'()*+,;=-._~', 'a b'],
    'host': [None, '', 'h', 'H.COM', 'h%41', 'h%zz', '1.2.3.4', '[::1]', '[::1', '::1', '[v1.x]', '[vF.X]', 'h:80', 'h/p', 'h@x', 'h#x', 'é', 'h!$&\'()*+,;=-._~', 'a b', '%5B::1%5D'],
    'port': [None, 0, 1, 80, 443, 65535, 65536, -1, 9223372036854775807],
    'path': ['', '/', 'a', '/a', '//a', '///a', 'a:b', '/a:b', 'a/b', '/a/../b', '/%41', '/%zz', '/a b', '/a?b', '/a#b', '/a[b', '/é', '/a@b', '/!$&\'()*+,;=:@-._~'],
    'query': [None, '', 'q', '?q', 'a=b&c=d', 'a?b', 'a#b', 'a/b', '%41', '%zz', 'a b', 'é', 'a[b]', '!$&\'()*+,;=:@-._~/?'],
    'fragment': [None, '', 'f', '#f', 'a?b', 'a#b', '%41', '%zz', 'a b', 'é', 'a[b]', '!$&\'()*+,;=:@-._~/?'],
}
for u in wuris:
    for op, args in wargs.items():
        for a in args:
            if op == 'port':
                emit('W', u, op, 'N' if a is None else ('I' + str(a)).encode())
            else:
                emit('W', u, op, 'N' if a is None else ('S'.encode() + (a.encode('utf-8') if isinstance(a, str) else a)))
