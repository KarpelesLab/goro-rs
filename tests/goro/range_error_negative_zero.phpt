--TEST--
range(): -0.0 is printed as 0.0 in its size error
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => range("9223372036854775808", -0.0, -2));
t(fn() => range(-0.0, 1e19, 1));
--EXPECT--
ValueError: The supplied range exceeds the maximum array size by 4611686017353646080.0 elements: start=0.0, end=9223372036854775808.0, step=2.0. Max size: 1073741824
ValueError: The supplied range exceeds the maximum array size by 9999999998926258176.0 elements: start=0.0, end=10000000000000000000.0, step=1.0. Max size: 1073741824
