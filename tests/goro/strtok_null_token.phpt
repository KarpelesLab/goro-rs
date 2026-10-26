--TEST--
strtok($s, null) is the one-argument form
--FILE--
<?php var_dump(strtok("abc")); var_dump(strtok("a b", null)); var_dump(strtok("a b c", " ")); var_dump(strtok(" ")); var_dump(strtok(" ", null)); var_dump(strtok(" ")); var_dump(strtok(" ")); var_dump(strtok(" ")); var_dump(strtok("x"));
--EXPECTF--

Warning: strtok(): Both arguments must be provided when starting tokenization in %s on line 1
bool(false)

Warning: strtok(): Both arguments must be provided when starting tokenization in %s on line 1
bool(false)
string(1) "a"
string(1) "b"
string(1) "c"
bool(false)
bool(false)
bool(false)
bool(false)
