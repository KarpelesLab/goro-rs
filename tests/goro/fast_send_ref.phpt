--TEST--
Fast by-reference sends (SEND_REF) from lean callees
--FILE--
<?php
function bump(&$x) { $x = ($x ?? 0) + 1; }
function setk(&$a, $k) { $a[$k] = true; }
function m1($s) { preg_match('/(b+)/', $s, $m); return $m; }
function m2($s) { $r = preg_match_all('/a/', $s, $m); return [$r, $m]; }
function undef_send() { bump($u); return $u; }
function ref_already(&$x) { bump($x); bump($x); return $x; }
function arr_elem() { $a = ['x' => 1]; bump($a['x']); bump($a['y']); return $a; }
function after_modify() { $v = 5; bump($v); $w = $v; $v = 100; return [$v, $w]; }
function shared_array() { $a = [1, 2]; $b = $a; setk($a, 'z'); return [$a, $b]; }
function sort_it($a) { sort($a); $c = $a; return $c; }
function swap(&$a, &$b) { [$a, $b] = [$b, $a]; }
function do_swap() { $p = 'a'; $q = 'b'; swap($p, $q); return "$p$q"; }
function nested_ref(&$arr) { bump($arr); return $arr; }
function caller_nested() { $z = 10; return nested_ref($z) + $z; }
for ($i = 0; $i < 40; $i++) {
    $r = [m1("abbbc"), m2("banana"), undef_send(), arr_elem(), after_modify(), shared_array(),
          sort_it([3, 1, 2]), do_swap(), caller_nested()];
    $t = 7;
    $r[] = ref_already($t);
    $r[] = $t;
}
echo json_encode($r), "\n";
function dumpref() { $a = 1; $b = &$a; bump($a); var_dump($b); $arr = [1]; bump($arr[0]); var_dump($arr); }
dumpref();
--EXPECT--
[["bbb","bbb"],[3,[["a","a","a"]]],1,{"x":2,"y":1},[100,6],[{"0":1,"1":2,"z":true},[1,2]],[1,2,3],"ba",22,9,9]
int(2)
array(1) {
  [0]=>
  int(2)
}
