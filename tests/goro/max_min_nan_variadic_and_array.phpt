--TEST--
max()/min() of several values and of an array: php's int/double loops and zend_hash_minmax() order with NAN
--FILE--
<?php
var_dump(max(1, NAN), max(NAN, 1), max(1.0, NAN), max(NAN, 1.0), min(1, NAN), min(NAN, 1), min(1.0, NAN), min(NAN, 1.0));
var_dump(max([1, NAN]), max([NAN, 1]), min([1, NAN]), min([NAN, 1]), max([1.0, NAN]), min([1.0, NAN]));
var_dump(max(1, 2, NAN), max(NAN, 2, 1), max(3, NAN, 2), max("1", NAN), max(NAN, "1"), max(true, NAN), max(INF, NAN), max(-INF, NAN));
var_dump(max(PHP_INT_MAX, (float)PHP_INT_MAX), max((float)PHP_INT_MAX, PHP_INT_MAX), min(PHP_INT_MAX, (float)PHP_INT_MAX), max(0, -0.0), max(-0.0, 0), min(0, -0.0), min(-0.0, 0), max(9007199254740993, 9007199254740992.0), max(9007199254740992.0, 9007199254740993));
var_dump(max([PHP_INT_MAX, (float)PHP_INT_MAX]), max([0, -0.0]), min([-0.0, 0]));
$vals = ['1' => 1, '1.0' => 1.0, 'NAN' => NAN, '2' => 2, '"1"' => "1", 'INF' => INF, '-1.0' => -1.0, '0' => 0];
$mx = 'max'; $mn = 'min';
foreach ($vals as $ka => $a) foreach ($vals as $kb => $b) {
  echo "$ka,$kb: ", var_export(max($a, $b), true), ' ', var_export($mx($a, $b), true), ' ', var_export(max([$a, $b]), true), ' | ', var_export(min($a, $b), true), ' ', var_export($mn($a, $b), true), ' ', var_export(min([$a, $b]), true), "\n";
}
foreach ([[1, 2, NAN], [NAN, 2, 1], [3, NAN, 2], [1.0, 2.0, NAN], [NAN, 1, 2], [1, NAN, 2], [2, NAN, 1], [1.5, NAN, 1], ["a", NAN, 1]] as $t) {
  echo json_encode(array_map(fn($x) => is_nan($x) ? 'NAN' : $x, $t)), ': ', var_export(max(...$t), true), ' ', var_export(max($t), true), ' ', var_export(min(...$t), true), ' ', var_export(min($t), true), "\n";
}
--EXPECTF--
float(NAN)
int(1)
float(NAN)
float(1)
float(NAN)
int(1)
float(NAN)
float(1)
int(1)
float(NAN)
float(NAN)
int(1)
float(1)
float(NAN)
int(2)
float(NAN)
int(3)
string(1) "1"
float(NAN)
bool(true)
float(NAN)
float(NAN)
int(9223372036854775807)
float(9.223372036854776E+18)
float(9.223372036854776E+18)
int(0)
float(-0)
float(-0)
int(0)
int(9007199254740993)
float(9007199254740992)
int(9223372036854775807)
int(0)
float(-0)
1,1: 1 1 1 | 1 1 1
1,1.0: 1 1 1 | 1.0 1 1
1,NAN: NAN 1 1 | NAN 1 NAN
1,2: 2 2 2 | 1 1 1
1,"1": 1 1 1 | '1' 1 1
1,INF: INF INF INF | 1 1 1
1,-1.0: 1 1 1 | -1.0 -1.0 -1.0
1,0: 1 1 1 | 0 0 0
1.0,1: 1.0 1.0 1.0 | 1 1.0 1.0
1.0,1.0: 1.0 1.0 1.0 | 1.0 1.0 1.0
1.0,NAN: NAN 1.0 1.0 | NAN 1.0 NAN
1.0,2: 2 2 2 | 1.0 1.0 1.0
1.0,"1": 1.0 1.0 1.0 | '1' 1.0 1.0
1.0,INF: INF INF INF | 1.0 1.0 1.0
1.0,-1.0: 1.0 1.0 1.0 | -1.0 -1.0 -1.0
1.0,0: 1.0 1.0 1.0 | 0 0 0
NAN,1: 1 NAN NAN | 1 NAN 1
NAN,1.0: 1.0 NAN NAN | 1.0 NAN 1.0
NAN,NAN: NAN NAN NAN | NAN NAN NAN
NAN,2: 2 NAN NAN | 2 NAN 2
NAN,"1": NAN '1' NAN | '1' NAN '1'
NAN,INF: INF NAN NAN | INF NAN INF
NAN,-1.0: -1.0 NAN NAN | -1.0 NAN -1.0
NAN,0: 0 NAN NAN | 0 NAN 0
2,1: 2 2 2 | 1 1 1
2,1.0: 2 2 2 | 1.0 1.0 1.0
2,NAN: NAN 2 2 | NAN 2 NAN
2,2: 2 2 2 | 2 2 2
2,"1": 2 2 2 | '1' '1' '1'
2,INF: INF INF INF | 2 2 2
2,-1.0: 2 2 2 | -1.0 -1.0 -1.0
2,0: 2 2 2 | 0 0 0
"1",1: '1' '1' '1' | 1 '1' '1'
"1",1.0: '1' '1' '1' | 1.0 '1' '1'
"1",NAN: '1' NAN '1' | NAN '1' NAN
"1",2: 2 2 2 | '1' '1' '1'
"1","1": '1' '1' '1' | '1' '1' '1'
"1",INF: INF INF INF | '1' '1' '1'
"1",-1.0: '1' '1' '1' | -1.0 -1.0 -1.0
"1",0: '1' '1' '1' | 0 0 0
INF,1: INF INF INF | 1 1 1
INF,1.0: INF INF INF | 1.0 1.0 1.0
INF,NAN: NAN INF INF | NAN INF NAN
INF,2: INF INF INF | 2 2 2
INF,"1": INF INF INF | '1' '1' '1'
INF,INF: INF INF INF | INF INF INF
INF,-1.0: INF INF INF | -1.0 -1.0 -1.0
INF,0: INF INF INF | 0 0 0
-1.0,1: 1 1 1 | -1.0 -1.0 -1.0
-1.0,1.0: 1.0 1.0 1.0 | -1.0 -1.0 -1.0
-1.0,NAN: NAN -1.0 -1.0 | NAN -1.0 NAN
-1.0,2: 2 2 2 | -1.0 -1.0 -1.0
-1.0,"1": '1' '1' '1' | -1.0 -1.0 -1.0
-1.0,INF: INF INF INF | -1.0 -1.0 -1.0
-1.0,-1.0: -1.0 -1.0 -1.0 | -1.0 -1.0 -1.0
-1.0,0: 0 0 0 | -1.0 -1.0 -1.0
0,1: 1 1 1 | 0 0 0
0,1.0: 1.0 1.0 1.0 | 0 0 0
0,NAN: NAN 0 0 | NAN 0 NAN
0,2: 2 2 2 | 0 0 0
0,"1": '1' '1' '1' | 0 0 0
0,INF: INF INF INF | 0 0 0
0,-1.0: 0 0 0 | -1.0 -1.0 -1.0
0,0: 0 0 0 | 0 0 0
[1,2,"NAN"]: 2 2 1 NAN
["NAN",2,1]: NAN NAN NAN 1
[3,"NAN",2]: 3 3 2 2
[1,2,"NAN"]: 2.0 2.0 1.0 NAN
["NAN",1,2]: NAN NAN NAN 1
[1,"NAN",2]: 2 2 1 2
[2,"NAN",1]: 2 2 1 1
[1.5,"NAN",1]: 1.5 1.5 1 1

Fatal error: Uncaught TypeError: is_nan(): Argument #1 ($num) must be of type float, string given in %s:13
Stack trace:
#0 %s(13): is_nan('a')
#1 [internal function]: {closure:%s:13}('a')
#2 %s(13): array_map(Object(Closure), Array)
#3 {main}
  thrown in %s on line 13
