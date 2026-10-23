--TEST--
ucwords()/addcslashes(): php_charmask() ranges and warnings
--FILE--
<?php
var_dump(ucwords("hello world", "a..z"));
var_dump(ucwords("a-b", ".."));
var_dump(ucwords("a-b c.d", "-..."));
var_dump(ucwords("hello_world-x", "_-"));
var_dump(addcslashes("abc..", "..c"));
var_dump(addcslashes("abc..", "a.."));
var_dump(addcslashes("abcd", "a..b..c"));
var_dump(addcslashes("abcz", "z..a"));
var_dump(addcslashes("abcz", "a..c"));
--EXPECTF--
string(11) "HeLlO wOrLd"

Warning: ucwords(): Invalid '..'-range, no character to the left of '..' in %s on line 3
string(3) "A-b"
string(7) "A-B c.D"
string(13) "Hello_World-X"

Warning: addcslashes(): Invalid '..'-range, no character to the left of '..' in %s on line 6
string(8) "ab\c\.\."

Warning: addcslashes(): Invalid '..'-range, no character to the right of '..' in %s on line 7
string(8) "\abc\.\."

Warning: addcslashes(): Invalid '..'-range in %s on line 8
string(7) "\a\b\cd"

Warning: addcslashes(): Invalid '..'-range, '..'-range needs to be incrementing in %s on line 9
string(6) "\abc\z"
string(7) "\a\b\cz"
