--TEST--
A compile-time fatal error in eval()'d code has no eval() frame
--FILE--
<?php
eval('function strlen() {}');
--EXPECTF--

Fatal error: Cannot redeclare function strlen() in %s(2) : eval()'d code on line 1
