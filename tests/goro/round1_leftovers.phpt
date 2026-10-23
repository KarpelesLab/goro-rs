--TEST--
mb_split() inside characters, mb_encode_mimeheader(null), iconv charset names, resource/Traversable argument checks, number_format(null)
--FILE--
<?php
function t($l, $f) { try { $r = $f(); echo "$l: ", json_encode($r), "\n"; } catch (\Throwable $e) { echo "$l: ", get_class($e), ": ", $e->getMessage(), "\n"; } }
t(1, fn() => mb_split('', 'äö'));
t(2, fn() => mb_split('$', 'ab'));
t(3, fn() => mb_split('', 'abc', 2));
t(4, fn() => mb_encode_mimeheader('héllo', null));
t(5, fn() => mb_encode_mimeheader('héllo', 'UTF-8', null));
t(6, fn() => iconv("\xff", "utf-8", "a"));
t(7, fn() => iconv("", "utf-8", "a"));
t(8, fn() => iconv("utf-8", "", "é"));
t(9, fn() => iconv("u t f 8", "utf-8", "é"));
t(10, fn() => iconv("bogus", "utf-8", "a"));
t(11, fn() => number_format(null));
t(12, fn() => iterator_apply([1], 'strlen'));
t(13, fn() => iterator_apply(new ArrayIterator([1])));
t(14, fn() => iterator_apply(new ArrayIterator([1, 2]), fn() => true));
t(15, fn() => get_resource_type(5));
t(16, fn() => get_resource_id("x"));
t(17, fn() => get_resource_type(STDIN));
t(18, fn() => get_resource_id(STDERR));
--EXPECTF--
1: ["\u00e4\u00f6"]

Warning: mb_split(): mbregex search failure in mbsplit(): no support in this configuration in %s on line 4
2: false
3: ["abc"]

Deprecated: mb_encode_mimeheader(): Passing null to parameter #2 ($charset) of type ?string is deprecated in %s on line 6
4: ValueError: mb_encode_mimeheader(): Argument #2 ($charset) must be a valid encoding, "" given

Deprecated: mb_encode_mimeheader(): Passing null to parameter #3 ($transfer_encoding) of type ?string is deprecated in %s on line 7
5: "=?UTF-8?B?aMOpbGxv?="
6: "a"
7: "a"
8: "\u00e9"
9: "\u00e9"

Warning: iconv(): Wrong encoding, conversion from "bogus" to "utf-8" is not allowed in %s on line 12
10: false

Deprecated: number_format(): Passing null to parameter #1 ($num) of type float is deprecated in %s on line 13
11: "0"
12: TypeError: iterator_apply(): Argument #1 ($iterator) must be of type Traversable, array given
13: ArgumentCountError: iterator_apply() expects at least 2 arguments, 1 given
14: 2
15: TypeError: get_resource_type(): Argument #1 ($resource) must be of type resource, int given
16: TypeError: get_resource_id(): Argument #1 ($resource) must be of type resource, string given
17: "stream"
18: 3
