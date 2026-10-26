--TEST--
str_pad() returns early before validating, substr_compare() zero length, strspn() huge length
--FILE--
<?php
// The input is returned unchanged before the pad arguments are validated.
var_dump(str_pad("abc", 2, ""));
var_dump(str_pad("abc", 3, "", 7));
var_dump(str_pad("abc", -1, "x", -1));
try { str_pad("abc", 4, ""); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
try { str_pad("abc", 4, "x", 7); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
// substr_compare(): an explicit length of 0 is 0, even for an offset past the end
var_dump(substr_compare("64", "abc", 7, 0, true));
try { substr_compare("64", "abc", 7, 1); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
// strspn()/strcspn() with a huge length
var_dump(strcspn("10", "123", 1, PHP_INT_MAX), strspn("1123", "1", 0, PHP_INT_MAX), strcspn("abc", "c", PHP_INT_MIN, PHP_INT_MIN));
--EXPECT--
string(3) "abc"
string(3) "abc"
string(3) "abc"
str_pad(): Argument #3 ($pad_string) must not be empty
str_pad(): Argument #4 ($pad_type) must be STR_PAD_LEFT, STR_PAD_RIGHT, or STR_PAD_BOTH
int(0)
substr_compare(): Argument #3 ($offset) must be contained in argument #1 ($haystack)
int(1)
int(2)
int(0)
