--TEST--
setlocale(): a non-Stringable object locale is a TypeError (zend_parse_arg_str())
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
class S { function __toString(): string { return "C"; } }
class T { function __toString(): string { throw new Exception("thrown"); } }
t(fn() => var_dump(setlocale(LC_ALL, new stdClass)));
t(fn() => var_dump(setlocale(LC_ALL, "C", new stdClass)));
t(fn() => var_dump(setlocale(LC_ALL, new S)));
t(fn() => var_dump(setlocale(LC_ALL, new T)));
--EXPECT--
TypeError: setlocale(): Argument #2 ($locales) must be of type array|string|null, stdClass given
TypeError: setlocale(): Argument #3 must be of type array|string|null, stdClass given
string(1) "C"
Exception: thrown
