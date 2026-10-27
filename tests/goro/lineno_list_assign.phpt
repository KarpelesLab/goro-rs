--TEST--
list() element fetches are on the line compiled last (key, else previous target)
--FILE--
<?php
$a = [];
[
    $x,
    $y
] = $a;
echo "--\n";
[$p,
    $q] = $a;
echo "--\n";
[
    'k1'
    =>
    $r,
    'k2' => $s
] = $a;
echo "--\n";
[$t, [$u,
  $v]] = [1, []];
echo "--\n";
$w =
    $undef;
$arr = [];
$arr['x'] =
    $undef2;
$o = new stdClass;
$o->p =
    $undef3;
--EXPECTF--

Warning: Undefined array key 0 in %s on line 4

Warning: Undefined array key 1 in %s on line 4
--

Warning: Undefined array key 0 in %s on line 8

Warning: Undefined array key 1 in %s on line 8
--

Warning: Undefined array key "k1" in %s on line 12

Warning: Undefined array key "k2" in %s on line 15
--

Warning: Undefined array key 0 in %s on line 18

Warning: Undefined array key 1 in %s on line 18
--

Warning: Undefined variable $undef in %s on line 21

Warning: Undefined variable $undef2 in %s on line 24

Warning: Undefined variable $undef3 in %s on line 27
