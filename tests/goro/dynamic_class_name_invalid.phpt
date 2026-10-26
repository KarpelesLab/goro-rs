--TEST--
$v::C / $v::$p / $x instanceof $v with $v neither an object nor a string: Class name must be a valid object or a string
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
foreach ([true, 1.5, [], 1, null, "Nope"] as $v) {
    t(fn() => $v::C);
    t(fn() => $v::$p);
    t(fn() => $v::m());
    t(fn() => new $v);
    t(fn() => $v instanceof $v);
    t(fn() => new stdClass instanceof $v);
}
--EXPECTF--
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class name must be a valid object or a string
Error: Class "Nope" not found
Error: Class "Nope" not found
Error: Class "Nope" not found
Error: Class "Nope" not found
bool(false)
bool(false)
