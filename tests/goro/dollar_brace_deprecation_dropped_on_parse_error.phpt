--TEST--
${ interpolation deprecations are not raised for a file that fails to parse
--FILE--
<?php
$var = 1;
$y = "${var}";
$x = 1 +;
--EXPECTF--

Parse error: syntax error, unexpected token ";" in %s on line 4
