--TEST--
preg: with pcre.jit a recursion loop exhausts the JIT stack
--FILE--
<?php
var_dump(preg_match("/(?R)?a/", "xa"), preg_last_error(), preg_last_error_msg());
var_dump(preg_match("/((?1)?)+/", "b"), preg_last_error_msg());
ini_set("pcre.jit", 0);
var_dump(preg_match("/(?R)?a/i", "xa"), preg_last_error(), preg_last_error_msg());
--EXPECT--
bool(false)
int(6)
string(25) "JIT stack limit exhausted"
bool(false)
string(25) "JIT stack limit exhausted"
bool(false)
int(1)
string(14) "Internal error"
