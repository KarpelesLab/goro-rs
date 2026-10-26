--TEST--
ArrayObject: a throwing offsetGet() in $ao[k] op= v skips an overriding offsetSet(); $ao[] op= v raises it
--FILE--
<?php
class AO extends ArrayObject { function offsetGet($k): mixed { echo "get "; var_dump($k); return 'p'; } }
$f = new AO([]);
try { $f[] .= 'x'; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($f->getArrayCopy());
class AO2 extends ArrayObject { function offsetGet($k): mixed { throw new Exception("get"); } }
$f = new AO2([]);
try { $f[] .= 'x'; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($f->getArrayCopy());
class AO3 extends ArrayObject { function offsetGet($k): mixed { throw new Exception("get"); } function offsetSet($k, $v): void { echo "set "; var_dump($k, $v); throw new Exception("set"); } }
$f = new AO3([]);
try { $f['a'] .= 'x'; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), " prev=", $e->getPrevious()?->getMessage(), "\n"; }
$f = new SplFixedArray(2);
class FA extends SplFixedArray { function offsetGet($k): mixed { throw new Exception("get"); } }
$f = new FA(2);
try { $f[0] .= 'x'; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($f->toArray());
--EXPECT--
ArgumentCountError: AO::offsetGet(): Argument #1 ($k) not passed
array(1) {
  [0]=>
  string(1) "x"
}
ArgumentCountError: AO2::offsetGet(): Argument #1 ($k) not passed
array(1) {
  [0]=>
  string(1) "x"
}
Exception: get prev=
Exception: get
array(2) {
  [0]=>
  string(1) "x"
  [1]=>
  NULL
}
