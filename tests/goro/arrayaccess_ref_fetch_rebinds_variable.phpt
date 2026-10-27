--TEST--
$r = &$r[k] of an ArrayAccess object binds $r to offsetGet()'s value, not writing it through the reference $r held
--FILE--
<?php
class C implements ArrayAccess {
    public function __construct(public array $items) {}
    public function offsetExists($k): bool { return isset($this->items[$k]); }
    public function offsetGet($k): mixed { return $this->items[$k]; }
    public function offsetSet($k, $v): void { $this->items[$k] = $v; }
    public function offsetUnset($k): void { unset($this->items[$k]); }
}
function forget(&$array, $key) {
    $original = &$array;
    $array = &$original;
    $parts = explode('.', $key);
    while (count($parts) > 1) {
        $part = array_shift($parts);
        $array = &$array[$part];
    }
    unset($array[array_shift($parts)]);
}
$items = [new C(['value', new C(['bar' => 'baz', 'test' => 'value'])]), 'bar'];
forget($items, '0.1.test');
var_dump($items);

$o = new C([1, new C([2])]);
$r = &$o;
$r = &$r[1];
var_dump($o === $r, $o->items[0]);
$o2 = new C([1, new C([2])]);
$r2 = &$o2[1];
var_dump($o2->items[0], $r2->items[0]);
$p = new C([1, new C([2])]);
$q = $p;
$q = &$q[1];
var_dump($p->items[0], $q->items[0]);

?>
--EXPECT--
array(2) {
  [0]=>
  object(C)#1 (1) {
    ["items"]=>
    array(2) {
      [0]=>
      string(5) "value"
      [1]=>
      object(C)#2 (1) {
        ["items"]=>
        array(1) {
          ["bar"]=>
          string(3) "baz"
        }
      }
    }
  }
  [1]=>
  string(3) "bar"
}
bool(false)
int(1)
int(1)
int(2)
int(1)
int(2)
