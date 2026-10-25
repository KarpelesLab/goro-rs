--TEST--
Functions verified against PHP 8.5 check their arguments (hebrev)
--FILE--
<?php
foreach ([fn() => hebrev(), fn() => hebrev([]), fn() => hebrev("x", "y")] as $f) {
    try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
}
--EXPECT--
ArgumentCountError: hebrev() expects at least 1 argument, 0 given
TypeError: hebrev(): Argument #1 ($string) must be of type string, array given
TypeError: hebrev(): Argument #2 ($max_chars_per_line) must be of type int, string given
