--TEST--
An early-bound class failing in eval()'d code has no eval() frame
--FILE--
<?php
function rec($n) { if ($n == 0) { eval('class W { abstract function f(); }'); } rec($n - 1); } rec(3);
--EXPECTF--

Fatal error: Class W declares abstract method f() and must therefore be declared abstract in %s(2) : eval()'d code on line 1
