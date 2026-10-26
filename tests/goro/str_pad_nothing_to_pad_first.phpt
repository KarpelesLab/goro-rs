--TEST--
str_pad(): no padding needed returns the input before the pad string / type checks
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => var_dump(str_pad("abc", 3, "")));
t(fn() => var_dump(str_pad("abc", 2, "", 99)));
t(fn() => var_dump(str_pad("abc", -1, "")));
t(fn() => var_dump(str_pad("abc", 4, "")));
t(fn() => var_dump(str_pad("abc", 4, "x", 99)));
--EXPECT--
string(3) "abc"
string(3) "abc"
string(3) "abc"
ValueError: str_pad(): Argument #3 ($pad_string) must not be empty
ValueError: str_pad(): Argument #4 ($pad_type) must be STR_PAD_LEFT, STR_PAD_RIGHT, or STR_PAD_BOTH
