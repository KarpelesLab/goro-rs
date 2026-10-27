--TEST--
GC: garbage destroyed slot by slot in root buffer order, handles recycled like Zend
--FILE--
<?php
class C {
    public $p;
    public function __construct() {
        $this->p = (object)["x" => [rand(1, 1)]];
    }
}
#[AllowDynamicProperties]
class E {
}
$e = new E;
$e->f = new E;
$e->f->e = $e;
$e->a = new C;
$e = null;
var_dump(gc_collect_cycles());
var_dump(new C);
// a ring of three with a tail of plain objects and arrays
$a = new E; $b = new E; $c = new E;
$a->n = $b; $b->n = $c; $c->n = $a;
$b->tail = [new E, new E];
$c->obj = new E;
$c->obj->deep = new E;
unset($a, $b, $c);
var_dump(gc_collect_cycles());
$ids = [];
for ($i = 0; $i < 8; $i++) { $o = new stdClass; $ids[] = spl_object_id($o); $keep[] = $o; }
echo implode(",", $ids), "\n";
// self cycle through an array
$x = new E; $x->arr = [$x, new E]; $x = null;
var_dump(gc_collect_cycles());
$n1 = new stdClass; $n2 = new stdClass;
echo spl_object_id($n1), ",", spl_object_id($n2), "\n";
--EXPECT--
int(5)
object(C)#1 (1) {
  ["p"]=>
  object(stdClass)#2 (1) {
    ["x"]=>
    array(1) {
      [0]=>
      int(1)
    }
  }
}
int(8)
1,2,5,4,3,6,7,8
int(3)
9,10
