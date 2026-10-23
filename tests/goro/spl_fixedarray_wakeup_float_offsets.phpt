--TEST--
SplFixedArray::__wakeup() deprecation and float offsets
--FILE--
<?php
$f = new SplFixedArray(3);
$f->p = 'a';
print_r($f); echo "\n";
var_dump($f);
$f->__wakeup();
try { $f[2.5e30] = 1; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump($f[NAN]); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $f[1.5] = 2; var_dump($f[1]); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(isset($f[INF])); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { unset($f[-INF]); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump($f->offsetGet(NAN)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECTF--

Deprecated: Creation of dynamic property SplFixedArray::$p is deprecated in %s on line 3
SplFixedArray Object
(
    [0] => 
    [1] => 
    [2] => 
    [p] => a
)

object(SplFixedArray)#1 (4) {
  [0]=>
  NULL
  [1]=>
  NULL
  [2]=>
  NULL
  ["p"]=>
  string(1) "a"
}

Deprecated: Method SplFixedArray::__wakeup() is deprecated since 8.4, this method is obsolete, as serialization hooks are provided by __unserialize() and __serialize() in %s on line 6

Warning: The float 2.5E+30 is not representable as an int, cast occurred in %s on line 7
OutOfBoundsException: Index invalid or out of range

Warning: The float NAN is not representable as an int, cast occurred in %s on line 8

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 8
NULL

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 9
int(2)

Warning: The float INF is not representable as an int, cast occurred in %s on line 10
bool(false)

Warning: The float -INF is not representable as an int, cast occurred in %s on line 11

Warning: The float NAN is not representable as an int, cast occurred in %s on line 12

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 12
NULL
