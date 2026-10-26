--TEST--
foreach by reference over a by-reference generator sees its keys
--FILE--
<?php
function &g() { $a = ['x' => 1, 'y' => 2]; foreach ($a as $k => &$v) yield $k => $v; }
foreach (g() as $k => &$v) { var_dump($k); }
function &keyed() { $x = 1; yield 3 => $x; yield $x; yield 'k' => $x; yield $x; }
foreach (keyed() as $k => &$v) echo var_export($k, true), " ";
echo "\n";
$g = keyed();
foreach ($g as $k => &$v) { if ($k === 4) break; }
var_dump($k, $g->key());
class Store { public $data = ['a' => 1, 'b' => 2]; function &items() { foreach ($this->data as $k => &$v) yield $k => $v; } }
$s = new Store;
foreach ($s->items() as $k => &$v) $v = "$k!";
unset($v);
var_dump($s->data);
--EXPECT--
string(1) "x"
string(1) "y"
3 4 'k' 5 
int(4)
int(4)
array(2) {
  ["a"]=>
  string(2) "a!"
  ["b"]=>
  string(2) "b!"
}
