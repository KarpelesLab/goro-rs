--TEST--
implode()/join() called as functions: the separator is array|string (scalars and Stringables coerced, others refused)
--FILE--
<?php function t($f) { try { var_dump($f()); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } } class S { function __toString(): string { return "-"; } } $f = "implode"; t(fn() => $f(new stdClass)); t(fn() => $f(new stdClass, [1])); t(fn() => $f(new S, [1, 2])); t(fn() => $f(5, [1, 2])); t(fn() => $f(1.5, [1, 2])); t(fn() => $f(true, [1, 2])); t(fn() => $f(STDIN, [1])); t(fn() => join(new stdClass, [1])); t(fn() => join(5, [1, 2])); t(fn() => implode(new S, [1, 2])); t(fn() => implode(new stdClass, [1]));
--EXPECTF--
TypeError: implode(): Argument #1 ($separator) must be of type array|string, stdClass given
TypeError: implode(): Argument #1 ($separator) must be of type array|string, stdClass given
string(3) "1-2"
string(3) "152"
string(5) "11.52"
string(3) "112"
TypeError: implode(): Argument #1 ($separator) must be of type array|string, resource given
TypeError: join(): Argument #1 ($separator) must be of type array|string, stdClass given
string(3) "152"
string(3) "1-2"
TypeError: implode(): Argument #1 ($separator) must be of type string, stdClass given
