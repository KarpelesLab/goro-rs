--TEST--
foreach by value over a by-reference generator binds the values, not the references
--FILE--
<?php
class P { function hi() { return "hi"; } }
class H extends stdClass implements IteratorAggregate {
    public function &getIterator(): Iterator {
        foreach ((array) $this as $key => $foo) {
            yield $key => $this->$key;
        }
    }
}
$h = new H;
$h->a = new P;
$h->b = 5;
foreach ($h as $k => $v) { echo $k, " ", is_object($v) ? $v->hi() : $v, "\n"; }
function &g() { $x = new P; yield $x; $arr = [1]; yield $arr[0]; }
foreach (g() as $v) { var_dump(is_object($v) ? $v->hi() : $v); }
foreach (g() as &$v) { var_dump(is_object($v) ? $v->hi() : $v); }
unset($v);
foreach ($h as $k => &$v) { if ($k === 'b') $v = 6; }
unset($v);
var_dump($h->b);
class H2 extends stdClass implements IteratorAggregate {
    public function &getIterator(): Iterator {
        foreach ((array) $this as $key => $foo) {
            yield $key => $this->$key;
        }
    }
}
function t() {
    $h = new H2;
    $h->a = 1;
    $h->b = 2;
    $h->c = 3;
    foreach ($h as $k => $v) { var_dump($v); }
    var_dump($h);
    $v = 9;
    var_dump($h);
}
t();
$f = function () { t(); };
$f();

?>
--EXPECT--
a hi
b 5
string(2) "hi"
int(1)
string(2) "hi"
int(1)
int(6)
int(1)
int(2)
int(3)
object(H2)#3 (3) {
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
  ["c"]=>
  int(3)
}
object(H2)#3 (3) {
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
  ["c"]=>
  int(3)
}
int(1)
int(2)
int(3)
object(H2)#5 (3) {
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
  ["c"]=>
  int(3)
}
object(H2)#5 (3) {
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
  ["c"]=>
  int(3)
}
