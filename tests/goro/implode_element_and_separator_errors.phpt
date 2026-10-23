--TEST--
implode(): elements are converted like strings, frameless separator errors
--FILE--
<?php
class S { function __toString(): string { return "-"; } }
try { var_dump(implode(",", [new stdClass])); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(implode(["a"], ",")); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(implode(",", [1, 2.5, true, false, null, "x"])); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(implode([new stdClass])); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(join(",", ["a", function(){}])); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(implode(",", ["a", [1]])); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(implode(1, ",")); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(join(["a"], ",")); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(implode(["a"], ",")); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(\implode(["a"], ",")); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(\implode(new \stdClass, [])); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(\implode(",", 5)); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(\implode(5)); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(\implode(new \stdClass)); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(implode(null, ["a","b"]));
var_dump(implode(new S, ["a","b"]));
var_dump(implode(1.5, ["a","b"]));
var_dump(implode(["a","b"]));
var_dump(implode(",", ["a", new S]));
try { var_dump(implode(",", ["a"], 3)); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(implode()); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(implode(",", null)); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(join(",", null)); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(join(null, [1])); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(join([1])); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(join("x")); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(join([1], [2])); } catch (\Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(implode(separator: ",", array: [1,2]));
--EXPECTF--
Error: Object of class stdClass could not be converted to string
TypeError: implode(): Argument #1 ($separator) must be of type string, array given
string(11) "1,2.5,1,,,x"
Error: Object of class stdClass could not be converted to string
Error: Object of class Closure could not be converted to string

Warning: Array to string conversion in %s on line 8
string(7) "a,Array"
TypeError: implode(): Argument #2 ($array) must be of type ?array, string given
TypeError: join(): Argument #2 ($array) must be of type ?array, string given
TypeError: implode(): Argument #1 ($separator) must be of type string, array given
TypeError: implode(): Argument #1 ($separator) must be of type string, array given
TypeError: implode(): Argument #1 ($separator) must be of type string, stdClass given
TypeError: implode(): Argument #2 ($array) must be of type ?array, int given
TypeError: implode(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given
TypeError: implode(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given

Deprecated: implode(): Passing null to parameter #1 ($separator) of type array|string is deprecated in %s on line 17
string(2) "ab"
string(3) "a-b"
string(5) "a1.5b"
string(2) "ab"
string(3) "a,-"
ArgumentCountError: implode() expects at most 2 arguments, 3 given
ArgumentCountError: implode() expects at least 1 argument, 0 given
TypeError: implode(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given
TypeError: join(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given

Deprecated: join(): Passing null to parameter #1 ($separator) of type array|string is deprecated in %s on line 26
string(1) "1"
string(1) "1"
TypeError: join(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given
TypeError: join(): Argument #1 ($separator) must be of type string, array given
string(3) "1,2"
