--TEST--
array_column() runs get hooks and initializes lazy objects; array_walk() skips virtual properties
--FILE--
<?php
class A { public $v { get => 99; } public $b = 2 { get => $this->b + 1; } } var_dump(array_column([new A], 'v'), array_column([new A], 'b'));
class B { public $id = 5; public $v { get => "vv"; } }
var_dump(array_column([new B], 'v', 'id'));
var_dump(array_column([new B], 'id', 'v'));
$r = new ReflectionClass(B::class);
$g = $r->newLazyGhost(function($o) { echo "init\n"; $o->id = 7; });
var_dump(array_column([$g], 'id'));
class W { public $a = 1; public $v { get => 2; } } $o = new W; array_walk($o, function($v, $k) { echo "$k=$v\n"; });
--EXPECT--
array(1) {
  [0]=>
  int(99)
}
array(1) {
  [0]=>
  int(3)
}
array(1) {
  [5]=>
  string(2) "vv"
}
array(1) {
  ["vv"]=>
  int(5)
}
init
array(1) {
  [0]=>
  int(7)
}
a=1
