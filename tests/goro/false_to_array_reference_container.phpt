--TEST--
No false-to-array deprecation for a reference container of a nested write
--FILE--
<?php
// FETCH_DIM_W converts a reference to false into an array silently; the
// assignment itself (ASSIGN_DIM) and plain variables still deprecate.
echo "1\n"; $c = false; $r = &$c; $c[0][1] = 1;
echo "2\n"; $d = false; $r2 = &$d; $d[0] = 1;
echo "3\n"; $e = false; $e[0][1] = 1;
echo "4\n"; $f = ['x' => false]; $g = &$f['x']; $f['x'][1] = 1;
echo "5\n"; $h = ['x' => false]; $i = &$h['x']; $h['x'][1][2] = 1;
echo "6\n"; $j = ['x' => false]; $j['x'][1][2] = 1;
echo "7\n"; function f() { global $k; $k[0][1] = 1; } $k = false; f();
echo "8\n"; function g() { static $s = false; $s[0][1] = 1; } g();
echo "9\n"; function h() { $c = false; $c[0][1] = 1; $d = false; $dr = &$d; $d[0][1] = 1; } h();
echo "10\n"; $u = false; $ur = &$u; $u[][1] = 1;
echo "11\n"; $c2 = false; $r3 = &$c2; $x = &$c2[0];
echo "12\n"; $d2 = false; $y = &$d2[0];
echo "13\n"; $GLOBALS['w'] = false; $w[0][1] = 1;
echo "done\n";
--EXPECTF--
1
2

Deprecated: Automatic conversion of false to array is deprecated in %s on line 5
3

Deprecated: Automatic conversion of false to array is deprecated in %s on line 6
4

Deprecated: Automatic conversion of false to array is deprecated in %s on line 7
5
6

Deprecated: Automatic conversion of false to array is deprecated in %s on line 9
7
8
9

Deprecated: Automatic conversion of false to array is deprecated in %s on line 12
10
11
12

Deprecated: Automatic conversion of false to array is deprecated in %s on line 15
13

Deprecated: Automatic conversion of false to array is deprecated in %s on line 16
done
