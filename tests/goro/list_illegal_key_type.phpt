--TEST--
List destructuring with an array or object key: TypeError
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(function () { [[$a] => $b] = []; });
t(function () { $o = new stdClass; [$o => $b] = [1]; });
--EXPECTF--

Warning: Undefined variable $a in %s on line 3
TypeError: Cannot access offset of type array on array
TypeError: Cannot access offset of type stdClass on array
