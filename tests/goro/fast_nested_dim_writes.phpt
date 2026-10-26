--TEST--
Fast nested writes into array elements and array properties
--FILE--
<?php
class H { public $callbacks = []; private $priv = ['a' => ['x' => 1]];
  function add($p, $id, $v) { $this->callbacks[$p][$id] = ['function' => $v, 'n' => (int)$p]; return count($this->callbacks); }
  function addp($k, $v) { $this->priv[$k][$v] = $v; return $this->priv; } }
class M { private $data = ['a' => ['b' => 1]]; function __get($n) { return $this->data; } }
$h = new H;
for ($i = 0; $i < 3; $i++) { $h->add(10, "id$i", "f$i"); $h->add(5, "id$i", [$i]); }
echo json_encode($h->callbacks), json_encode($h->addp('a', 'y')), json_encode($h->addp('b', 'z')), "\n";
$c = $h->callbacks;
$h->add(10, 'idx', 'g');
echo json_encode($c), json_encode($h->callbacks), "\n";
function m() { $m = new M; try { $m->data['a']['c'] = 2; } catch (Error $e) { echo $e->getMessage(), "\n"; } var_dump($m->data); }
m();
class N { public array $t = []; public ?array $n = null; }
function n() { $o = new N; $o->t['a']['b'] = 1; $o->t['a']['c'] = 2; $o->n['x']['y'] = 3; $r = &$o->t['a']; $o->t['a']['d'] = 4; return [$o, $r]; }
echo json_encode(n()), "\n";
--EXPECTF--
{"10":{"id0":{"function":"f0","n":10},"id1":{"function":"f1","n":10},"id2":{"function":"f2","n":10}},"5":{"id0":{"function":[0],"n":5},"id1":{"function":[1],"n":5},"id2":{"function":[2],"n":5}}}{"a":{"x":1,"y":"y"}}{"a":{"x":1,"y":"y"},"b":{"z":"z"}}
{"10":{"id0":{"function":"f0","n":10},"id1":{"function":"f1","n":10},"id2":{"function":"f2","n":10}},"5":{"id0":{"function":[0],"n":5},"id1":{"function":[1],"n":5},"id2":{"function":[2],"n":5}}}{"10":{"id0":{"function":"f0","n":10},"id1":{"function":"f1","n":10},"id2":{"function":"f2","n":10},"idx":{"function":"g","n":10}},"5":{"id0":{"function":[0],"n":5},"id1":{"function":[1],"n":5},"id2":{"function":[2],"n":5}}}

Notice: Indirect modification of overloaded property M::$data has no effect in %s on line 12
array(1) {
  ["a"]=>
  array(1) {
    ["b"]=>
    int(1)
  }
}
[{"t":{"a":{"b":1,"c":2,"d":4}},"n":{"x":{"y":3}}},{"b":1,"c":2,"d":4}]
