--TEST--
Fast paths: references of variables, by-reference sends of calls resolved at run time, instanceof sites seeing several classes
--FILE--
<?php
class K { function get($k, &$found = null) { $found = $k > 1; return $k * 2; } }
interface I {} class P implements I {} class Q {} class R extends P {}
function inst($o) { return [$o instanceof P, $o instanceof I, $o instanceof Q]; }
function cache($o, $k) { $x = $o->get($k, $found); return [$x, $found]; }
function cache2($o, $k) { $found = 'old'; $o->get($k, $found); return $found; }
function cache3($o, $k) { $f = &$g; $o->get($k, $f); return [$f, $g]; }
function pa($args) { $p = &$args; $p[] = 1; return [$p, $args]; }
function pa2($args) { $p = 5; $p = &$args; $q = &$args; $q = 3; return [$p, $args, $q]; }
function pa3() { $p = &$undef; var_dump($undef); return $p; }
function pa4($o) { $p = &$o; $p = 'x'; return $o; }
$o = new K;
for ($i = 0; $i < 3; $i++) {
  echo json_encode(cache($o, $i)), json_encode(cache2($o, $i)), json_encode(cache3($o, $i)), "\n";
  echo json_encode(pa([$i])), json_encode(pa2([$i])), json_encode(pa3()), json_encode(pa4($o)), "\n";
  foreach ([new P, new Q, new R, new K, new P, new Q] as $x) echo json_encode(inst($x));
  echo "\n";
}
function g() { static $s = 1; $t = &$s; $t++; return $s; }
echo g(), g(), g(), "\n";
--EXPECT--
[0,false]false[false,false]
[[0,1],[0,1]][3,3,3]NULL
null"x"
[true,true,false][false,false,true][true,true,false][false,false,false][true,true,false][false,false,true]
[2,false]false[false,false]
[[1,1],[1,1]][3,3,3]NULL
null"x"
[true,true,false][false,false,true][true,true,false][false,false,false][true,true,false][false,false,true]
[4,true]true[true,true]
[[2,1],[2,1]][3,3,3]NULL
null"x"
[true,true,false][false,false,true][true,true,false][false,false,false][true,true,false][false,false,true]
234
