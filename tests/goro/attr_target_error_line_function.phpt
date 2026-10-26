--TEST--
Attribute target error: reported at the function's line
--FILE--
<?php

#[\Override]
function f(){}
--EXPECTF--

Fatal error: Attribute "Override" cannot target function (allowed targets: method, property) in %s on line 4
