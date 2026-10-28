#!/usr/bin/env python3
"""Builds whatwg_setter_cases.txt: Uri\\WhatWg\\Url withers over the WPT
setters_tests.json cases (BSD-3-Clause, web-platform-tests) and hand-written
ones.  Line: <hex url>\\t<method>\\t<arg>  (arg: N, I<int>, or S<hex>)."""
import json, sys

D = sys.argv[1]
out = open(sys.argv[2], 'w')
seen = set()

def h(s):
    return s.encode('utf-8', 'surrogatepass').hex()

def emit(url, method, arg):
    if arg is None:
        a = 'N'
    elif isinstance(arg, int):
        a = 'I' + str(arg)
    else:
        a = 'S' + h(arg)
    key = (url, method, a)
    if key in seen:
        return
    seen.add(key)
    out.write(f"{h(url)}\t{method}\t{a}\n")

M = {'protocol': 'withScheme', 'username': 'withUsername', 'password': 'withPassword',
     'hostname': 'withHost', 'host': 'withHost', 'pathname': 'withPath', 'search': 'withQuery',
     'hash': 'withFragment'}
data = json.load(open(f"{D}/setters_tests.json"))
for key, tests in data.items():
    if key == 'comment' or not isinstance(tests, list):
        continue
    for t in tests:
        if not isinstance(t, dict):
            continue
        href, v = t['href'], t['new_value']
        if key == 'port':
            vv = v.strip()
            if vv.isdigit():
                emit(href, 'withPort', int(vv))
            emit(href, 'withPort', None)
            continue
        if key == 'href':
            continue
        emit(href, M[key], v)
        if key in ('search', 'hash', 'username', 'password', 'hostname'):
            emit(href, M[key], None)

bases = ['https://example.com/p', 'foo://example.com/p', 'foo://u@example.com/p', 'foo://example.com:81/p',
         'foo:bar', 'file://h/p', 'foo:/p', 'https://u:p@example.com:8080/x', 'file:///x', 'foo:///x',
         'foo:bar  ', 'foo:bar  #x', ' https://u:p@h/ ', 'https:\\\\u:p@h/', "http://h/\tx", 'http://[::1]/',
         'http://1.2.3.4:5/', 'ws://w/', 'wss://w:443/', 'ftp://f:21/', 'http://xn--ls8h.com/', 'sc://é/']
args = {
    'withScheme': ['foo', 'ftp', 'file', 'http', 'https', 'ws', '1a', '', 'a b', 'h:t', 'HTTP:', 'ws://x', 'sc', 'Foo+Bar', 'a\tb', '\tfoo', 'foo\n'],
    'withUsername': [None, '', 'x', 'u:p', 'é', '@', ' ', '%41', '\x00', 'a\tb'],
    'withPassword': [None, '', 'x', 'u:p', 'é', '@', ' ', '%41', '\x00', 'a\tb'],
    'withHost': [None, '', 'h', 'h:1', 'h/x', 'h?x', '[::1]', '[::1', 'é', 'a b', '%', 'xn--', '1.2.3.4.5', 'H.COM', '0x7f.1', 'h\\x', 'h@x', 'h#x', '%41', 'a\tb', '\th', '256', '[v1.x]', 'x.­', 'xn--zz'],
    'withPort': [None, 0, 1, 80, 443, 21, 65535, 65536, -1, 9223372036854775807],
    'withPath': ['', '/', 'a', '/a/../b', '\\a\\b', '?', '#', '%zz', 'é', ' ', 'a/./', '..', 'C:', 'C|/x', '//x', '/a/..?x', 'a\tb', '/.//x'],
    'withQuery': [None, '', '?', '??', 'a b', '#', "'", '%zz', 'é', '\x00', 'a#b', 'a\tb', '\t'],
    'withFragment': [None, '', '#', '##', 'a b', '?', "'", '%zz', 'é', '\x00', 'a#b', 'a\tb', '`', '<>'],
}
for b in bases:
    for m, vals in args.items():
        for v in vals:
            emit(b, m, v)
