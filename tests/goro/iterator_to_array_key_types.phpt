--TEST--
iterator_to_array() stores keys like array_set_zval_key()
--FILE--
<?php
$it=new RegexIterator(new ArrayIterator([10=>1, 'a1'=>2, 'x' => 3]),"/^\\d/",RegexIterator::REPLACE,RegexIterator::USE_KEY);
var_dump(iterator_to_array($it,true));
function g() { yield "5" => 1; yield 1.5 => 2; yield true => 3; yield null => 4; yield "07" => 5; }
var_dump(iterator_to_array(g()));
class K implements Iterator {
    private $i = 0; private $k;
    function __construct($k) { $this->k = $k; }
    function rewind(): void { $this->i = 0; }
    function valid(): bool { return $this->i < count($this->k); }
    function current(): mixed { return $this->i; }
    function key(): mixed { return $this->k[$this->i]; }
    function next(): void { $this->i++; }
}
var_dump(iterator_to_array(new K(["5", 2.0, 2.5, false, null, "-3", "x"])));
try { var_dump(iterator_to_array(new K(["a", [1]]))); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(iterator_to_array(new K(["a", new stdClass]))); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(iterator_count(new K([1,2,3])));
--EXPECTF--
array(1) {
  [0]=>
  int(1)
}

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 5

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 5
array(4) {
  [5]=>
  int(1)
  [1]=>
  int(3)
  [""]=>
  int(4)
  ["07"]=>
  int(5)
}

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 15

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 15
array(6) {
  [5]=>
  int(0)
  [2]=>
  int(2)
  [0]=>
  int(3)
  [""]=>
  int(4)
  [-3]=>
  int(5)
  ["x"]=>
  int(6)
}
TypeError: Cannot access offset of type array on array
TypeError: Cannot access offset of type stdClass on array
int(3)
