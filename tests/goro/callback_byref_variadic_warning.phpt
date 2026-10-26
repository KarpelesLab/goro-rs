--TEST--
A by-reference variadic argument given a value by a callback call: the warning names no parameter
--FILE--
<?php var_dump(call_user_func("sscanf", "12 34", "%d %d", 1, 2)); var_dump(call_user_func_array("sscanf", ["12", "%d", 5])); var_dump(call_user_func("str_replace", "a", "b", "aa", 0)); var_dump(call_user_func("preg_match", "/a/", "a", 0)); var_dump(call_user_func("array_multisort", [2, 1], [3, 4]));
--EXPECTF--

Warning: sscanf(): Argument #3 must be passed by reference, value given in %s on line 1

Warning: sscanf(): Argument #4 must be passed by reference, value given in %s on line 1
int(2)

Warning: sscanf(): Argument #3 must be passed by reference, value given in %s on line 1
int(1)

Warning: str_replace(): Argument #4 ($count) must be passed by reference, value given in %s on line 1
string(2) "bb"

Warning: preg_match(): Argument #3 ($matches) must be passed by reference, value given in %s on line 1
int(1)
bool(true)
