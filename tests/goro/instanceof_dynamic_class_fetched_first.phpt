--TEST--
instanceof fetches a dynamic class before reading its operand
--FILE--
<?php
try { var_dump($u10 instanceof $u11); } catch (Error $e) { echo $e->getMessage(), "\n"; }
$cls = 'stdClass';
var_dump($u12 instanceof $cls);
set_error_handler(function($n, $s) { throw new Exception("handler: $s"); });
try { var_dump($u13 instanceof $u14); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECTF--

Warning: Undefined variable $u11 in %s on line 2
Class name must be a valid object or a string

Warning: Undefined variable $u12 in %s on line 4
bool(false)
Exception: handler: Undefined variable $u14
