--TEST--
Undefined key variables of isset()/empty()/?? dimension chains warn at their level (also below a missing one); ??= reads its value after the write's key
--FILE--
<?php
$a = [];
$a[$u1] ??= $u2;
echo "--\n";
$a[$u3][$u4] ??= 5;
echo "--\n";
$a[$u5] ??= 7;
var_dump($a);
$a = [];
$a[$v1] = $v2;
echo "--\n";
$a[$v3][$v4] = $v5;
echo "--\n";
$b = [];
$b[$v6][$v7] ??= $v8;
echo "--\n";
$o = new stdClass;
$o->{$v9} ??= $v10;
echo "--\n";
$c = ['' => null];
$c[$v11][$v12] ??= 5;
$c = ['' => null];
var_dump($c[$x11][$x12] ?? 5);
var_dump(isset($c[$x1][$x2]));
var_dump(empty($c[$x3][$x4]));
$d = ['' => ['' => 1]];
var_dump($d[$x5][$x6][$x7] ?? 5);
$y = ($a[$x8] = $x9);
var_dump($y);
var_dump(isset($undefarr[$x10][$x11]), empty($undefarr[$x12]['k'][$x13]), $undefarr[$x14][$x15] ?? 'd');
--EXPECTF--

Warning: Undefined variable $u1 in %s on line 3

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 3

Warning: Undefined variable $u1 in %s on line 3

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 3

Warning: Undefined variable $u2 in %s on line 3
--

Warning: Undefined variable $u3 in %s on line 5

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 5

Warning: Undefined variable $u4 in %s on line 5

Warning: Undefined variable $u3 in %s on line 5

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 5

Warning: Undefined variable $u4 in %s on line 5

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 5
--

Warning: Undefined variable $u5 in %s on line 7

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 7
array(1) {
  [""]=>
  array(1) {
    [""]=>
    int(5)
  }
}

Warning: Undefined variable $v1 in %s on line 10

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 10

Warning: Undefined variable $v2 in %s on line 10
--

Warning: Undefined variable $v3 in %s on line 12

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 12

Warning: Undefined variable $v4 in %s on line 12

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 12

Warning: Undefined variable $v5 in %s on line 12
--

Warning: Undefined variable $v6 in %s on line 15

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 15

Warning: Undefined variable $v7 in %s on line 15

Warning: Undefined variable $v6 in %s on line 15

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 15

Warning: Undefined variable $v7 in %s on line 15

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 15

Warning: Undefined variable $v8 in %s on line 15
--

Warning: Undefined variable $v9 in %s on line 18

Warning: Undefined variable $v10 in %s on line 18

Warning: Undefined variable $v9 in %s on line 18
--

Warning: Undefined variable $v11 in %s on line 21

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 21

Warning: Undefined variable $v12 in %s on line 21

Warning: Undefined variable $v11 in %s on line 21

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 21

Warning: Undefined variable $v12 in %s on line 21

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 21

Warning: Undefined variable $x11 in %s on line 23

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 23

Warning: Undefined variable $x12 in %s on line 23
int(5)

Warning: Undefined variable $x1 in %s on line 24

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 24

Warning: Undefined variable $x2 in %s on line 24
bool(false)

Warning: Undefined variable $x3 in %s on line 25

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 25

Warning: Undefined variable $x4 in %s on line 25
bool(true)

Warning: Undefined variable $x5 in %s on line 27

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 27

Warning: Undefined variable $x6 in %s on line 27

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 27

Warning: Undefined variable $x7 in %s on line 27
int(5)

Warning: Undefined variable $x8 in %s on line 28

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 28

Warning: Undefined variable $x9 in %s on line 28
NULL

Warning: Undefined variable $x10 in %s on line 30

Warning: Undefined variable $x11 in %s on line 30

Warning: Undefined variable $x12 in %s on line 30

Warning: Undefined variable $x13 in %s on line 30

Warning: Undefined variable $x14 in %s on line 30

Warning: Undefined variable $x15 in %s on line 30
bool(false)
bool(true)
string(1) "d"
