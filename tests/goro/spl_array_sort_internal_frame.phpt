--TEST--
ArrayObject sort methods call the sort function through an internal frame
--INI--
zend.exception_ignore_args=0
--FILE--
<?php
$ao = new ArrayObject([2, 1]);
try { $ao->uasort('nonexistent'); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $ao->asort(100); var_dump($ao->getArrayCopy()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $ao->asort('x'); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $ao->uksort(function($a, $b) { return debug_backtrace(); }); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$ao->uasort(function($a, $b) { echo count(debug_backtrace()), " ", debug_backtrace()[1]['function'], "\n"; return $a <=> $b; });
$ao->uasort(fn($a, $b) => 'x');
var_dump($ao->getArrayCopy());
try { $ao->uasort(function($a, $b) { throw new Exception('cmp'); }); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
?>
--EXPECTF--
TypeError: uasort(): Argument #2 ($callback) must be a valid callback, function "nonexistent" not found or invalid function name
array(2) {
  [1]=>
  int(1)
  [0]=>
  int(2)
}
TypeError: ArrayObject::asort(): Argument #1 ($flags) must be of type int, string given
3 uasort
array(2) {
  [1]=>
  int(1)
  [0]=>
  int(2)
}
#0 [internal function]: {closure:%s:10}(1, 2)
#1 [internal function]: uasort(Array, Object(Closure))
#2 %s(10): ArrayObject->uasort(Object(Closure))
#3 {main}
