--TEST--
get_defined_functions(), function_exists() and ReflectionFunction don't see the engine's internal helpers or bound closures
--FILE--
<?php
function __mine() {}
$f = function() { return 1; };
$g = Closure::bind($f, new stdClass);
class K { const C = 1 + 2; }
var_dump(K::C);
var_dump(get_defined_functions()["user"]);
var_dump(count(array_filter(get_defined_functions()["internal"], fn($n) => str_starts_with($n, "__"))));
var_dump(function_exists("__declare_const"), function_exists("__phar_m"));
try { new ReflectionFunction("__bcmath_number"); } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
?>
--EXPECT--
int(3)
array(1) {
  [0]=>
  string(6) "__mine"
}
int(0)
bool(false)
bool(false)
Function __bcmath_number() does not exist
