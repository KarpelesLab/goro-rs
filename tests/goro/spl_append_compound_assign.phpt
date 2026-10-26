--TEST--
SPL: `$obj[] op= v` reads through the read_dimension handler without an offset
--FILE--
<?php
function t($f) { try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; if ($e->getPrevious()) echo "  prev: ", $e->getPrevious()->getMessage(), "\n"; } }
t(function() { $f = new ArrayObject([1]); $f[] .= 'x'; $f[] += 2; var_dump($f->getArrayCopy()); });
t(function() { $f = new ArrayIterator([]); $f[] .= 'x'; var_dump($f->getArrayCopy()); });
t(function() { $f = new SplFixedArray(2); $f[] .= 'x'; });
t(function() { $f = new SplObjectStorage; $f[] .= 'x'; });
class AO extends ArrayObject { function offsetGet($k): mixed { echo "get "; var_dump($k); return 'p'; } }
t(function() { $f = new AO([]); $f[] .= 'x'; });
class AO2 extends ArrayObject { function offsetGet($k): mixed { throw new Exception("get $k"); } }
t(function() { $f = new AO2([]); $f['k'] .= 'x'; });
class AA implements ArrayAccess { function offsetGet($k): mixed { echo "get "; var_dump($k); return 'p'; } function offsetSet($k, $v): void { echo "set "; var_dump($k, $v); } function offsetExists($k): bool { return true; } function offsetUnset($k): void {} }
t(function() { $f = new AA; $f[] .= 'x'; });
?>
--EXPECT--
array(3) {
  [0]=>
  int(1)
  [1]=>
  string(1) "x"
  [2]=>
  int(2)
}
array(1) {
  [0]=>
  string(1) "x"
}
Error: Cannot use object of type SplFixedArray as array
  prev: [] operator not supported for SplFixedArray
Error: Cannot use object of type SplObjectStorage as array
  prev: SplObjectStorage::offsetGet(): Argument #1 ($object) must be of type object, null given
ArgumentCountError: AO::offsetGet(): Argument #1 ($k) not passed
Exception: get k
get NULL
set NULL
string(2) "px"
