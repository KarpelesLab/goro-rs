--TEST--
pack()/unpack(): repeaters, @/X/x positioning, nibble codes, per-code numeric keys
--FILE--
<?php
// pack()/unpack(): a port of ext/standard/pack.c
$p = [
    ['H3', 'abcdef'], ['h3', 'abcdef'], ['H*', 'abc'], ['@4'], ['Ca3@6C', 1, 'x', 2],
    ['C2@1C', 1, 2, 3], ['a2X', 'ab'], ['X'], ['Z2', 'abc'], ['Z*', 'ab'], ['A5', 'ab'],
    ['nvc*', 1, 2, 3, 4], ['J', 0x0102030405060708], ['x3'], ['C', 1, 2],
];
foreach ($p as $a) {
    $f = array_shift($a);
    echo $f, ': ', bin2hex(pack($f, ...$a)), "\n";
}
foreach ([['Y', 1], ['N'], ['A']] as $a) {
    $f = array_shift($a);
    try { pack($f, ...$a); } catch (\ValueError $e) { echo $e->getMessage(), "\n"; }
}
var_dump(unpack('N', 'ab'));
var_dump(unpack('C2x/C', 'abcd'));
var_dump(unpack('C2a/C2b', 'abcd'));
var_dump(unpack('@2/C', 'abcd'));
var_dump(unpack('Cfirst/Csecond/x/C*rest', 'abcdef'));
var_dump(unpack('H3', "\xab\xcd"));
var_dump(unpack('a2x/A*y', "ab cd  \0"));
var_dump(unpack('C5', 'abc'));
var_dump(unpack('c*', "\xff\x80"));
var_dump(unpack('X3/C', 'ab'));
var_dump(unpack('@9/C', 'ab'));
var_dump(unpack('C0a/Cb', 'xy'));
var_dump(unpack('C2/C', 'abc'));
var_dump(unpack('Ca/C1b/C2c', 'abcd'));
var_dump(unpack('C1/C', 'ab', 1));
try { unpack('Q2', 'x'); unpack('y', 'x'); } catch (\ValueError $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
H3: abc0
h3: ba0c
H*: abc0
@4: 00000000
Ca3@6C: 01780000000002
C2@1C: 0103
a2X: 61
X: 
Warning: pack(): Type X: outside of string in %s on line 10

Z2: 6100
Z*: 616200
A5: 6162202020
nvc*: 000102000304
J: 0102030405060708
x3: 000000
C: 
Warning: pack(): 1 arguments unused in %s on line 10
01
Type Y: unknown format code
Type N: too few arguments
Type A: not enough arguments

Warning: unpack(): Type N: not enough input values, need 4 values but only 2 were provided in %s on line 16
bool(false)
array(3) {
  ["x1"]=>
  int(97)
  ["x2"]=>
  int(98)
  [1]=>
  int(99)
}
array(4) {
  ["a1"]=>
  int(97)
  ["a2"]=>
  int(98)
  ["b1"]=>
  int(99)
  ["b2"]=>
  int(100)
}
array(1) {
  [1]=>
  int(99)
}
array(5) {
  ["first"]=>
  int(97)
  ["second"]=>
  int(98)
  ["rest1"]=>
  int(100)
  ["rest2"]=>
  int(101)
  ["rest3"]=>
  int(102)
}
array(1) {
  [1]=>
  string(3) "abc"
}
array(2) {
  ["x"]=>
  string(2) "ab"
  ["y"]=>
  string(3) " cd"
}

Warning: unpack(): Type C: not enough input values, need 1 values but only 0 were provided in %s on line 23
bool(false)
array(2) {
  [1]=>
  int(-1)
  [2]=>
  int(-128)
}
array(1) {
  [1]=>
  int(97)
}

Warning: unpack(): Type @: outside of string in %s on line 26
array(1) {
  [1]=>
  int(97)
}
array(1) {
  ["b"]=>
  int(120)
}
array(2) {
  [1]=>
  int(99)
  [2]=>
  int(98)
}
array(4) {
  ["a"]=>
  int(97)
  ["b"]=>
  int(98)
  ["c1"]=>
  int(99)
  ["c2"]=>
  int(100)
}

Warning: unpack(): Type C: not enough input values, need 1 values but only 0 were provided in %s on line 30
bool(false)

Warning: unpack(): Type Q: not enough input values, need 8 values but only 1 was provided in %s on line 31
Invalid format type y
