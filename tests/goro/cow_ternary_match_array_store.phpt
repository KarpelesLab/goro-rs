--TEST--
A variable's array taken by ?: / match / ?? and stored in a property or element is a copy
--FILE--
<?php
function h1(array $a) { $o = new stdClass; $o->p = $a; sort($a); return $o->p[0]; }
function h2(array $a) { $o = new stdClass; $o->p = true ? $a : []; sort($a); return $o->p[0]; }
function h3(array $a) { $o = new stdClass; $o->p = match (1) { 1 => $a }; sort($a); return $o->p[0]; }
function h4(array $a) { $o = new stdClass; $o->p = match (1) { 1 => $a }; $a[] = 'z'; rsort($a); return $o->p[0]; }
function h5(array $a) { $o = []; $o['p'] = match (1) { 1 => $a }; sort($a); return $o['p'][0]; }
function h6(array $a) { $x = match (1) { 1 => $a }; sort($a); return $x[0]; }
function h7(array $a) { $o = new stdClass; $o->p = match (1) { 1 => $a }; $a[0] = 'q'; return $o->p[0]; }
function h8(array $a) { $o = new stdClass; $o->p = $a ?: []; sort($a); return $o->p[0]; }
function h9(array $a) { $o = new stdClass; $o->p = $a ?? []; sort($a); return $o->p[0]; }
function h10(array $a) { $o = []; $o[] = true ? $a : []; $a[0] = 'q'; return $o[0][0]; }
function h11(array $a) { static $s; $s = true ? $a : []; $a[0] = 'q'; return $s[0]; }
function h12(array $a) { $o = new stdClass; $o->p = (true ? $a : []); $o->p[0] = 'q'; return $a[0]; }
function h13(array $a) { $r = [true ? $a : []]; $a[0] = 'q'; return $r[0][0]; }
class S { public static $p; }
function h14(array $a) { S::$p = true ? $a : []; $a[0] = 'q'; return S::$p[0]; }
foreach (['h1','h2','h3','h4','h5','h6','h7','h8','h9','h10','h11','h12','h13','h14'] as $f) echo $f, ' ', $f(['c', 'a', 'b']), "\n";
$g = ['c', 'a', 'b']; $o = new stdClass; $o->p = true ? $g : []; $g[0] = 'z'; echo $o->p[0], "\n";

?>
--EXPECT--
h1 c
h2 c
h3 c
h4 c
h5 c
h6 c
h7 c
h8 c
h9 c
h10 c
h11 c
h12 c
h13 c
h14 c
c
