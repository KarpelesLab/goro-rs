--TEST--
array_merge, implode and call_user_func by name: results, element copies and references
--FILE--
<?php
class S { function __toString(): string { return "s"; } }
function show($v) { echo json_encode($v), "\n"; }
$x = 1; $nested = [1, 2];
$a = ["k" => 1, 5 => "a", "r" => &$x, "n" => $nested];
$b = [5 => "b", "k" => 2, 9 => [3]];
$m = array_merge($a, $b, [], ["z" => null]);
show($m);
$x = 7;
show($m["r"]);
$m["n"][] = 3;
show($nested);
show(array_merge());
show(array_merge([3 => 1], [3 => 2]));
show(array_merge(["a" => 1], ["a" => 2, "b" => 3], ["b" => 4]));
try { array_merge([1], 2); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
show(array_merge_recursive(["a" => [1], "b" => 2], ["a" => [3], "b" => 4]));
$self = [1, 2];
show(array_merge($self, $self));
show(implode(",", ["a", "b", "c"]));
show(implode(", ", [1, 2.5, true, false, null, "x", new S]));
show(implode(["a", "b"]));
show(implode("", []));
show(implode("-", ["only"]));
show(implode("--", ["", "", ""]));
echo implode(",", [[1], "a"]), "\n";
show(join("+", ["j", 1]));
function cb_upper($s) { return strtoupper($s); }
class K { static function st($v) { return "st:$v"; } function m($v) { return "m:$v"; } }
show(call_user_func('cb_upper', "abc"));
show(call_user_func('\cb_upper', "abc"));
show(call_user_func('CB_UPPER', "abc"));
show(call_user_func('strtoupper', "abc"));
show(call_user_func('\STRTOUPPER', "abc"));
show(call_user_func('K::st', 1));
show(call_user_func('k::ST', 2));
show(call_user_func(['K', 'st'], 3));
show(call_user_func([new K, 'm'], 4));
show(call_user_func_array('cb_upper', ["xyz"]));
show(array_map('cb_upper', ["a", "b"]));
show(array_map('K::st', [5]));
try { call_user_func('no_such_fn'); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { call_user_func('K::nope'); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { call_user_func('a:b'); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
?>
--EXPECTF--
{"k":2,"0":"a","r":1,"n":[1,2],"1":"b","2":[3],"z":null}
7
[1,2]
[]
[1,2]
{"a":2,"b":4}
array_merge(): Argument #2 must be of type array, int given
{"a":[1,3],"b":[2,4]}
[1,2,1,2]
"a,b,c"
"1, 2.5, 1, , , x, s"
"ab"
""
"only"
"----"

Warning: Array to string conversion in %s on line 26
Array,a
"j+1"
"ABC"
"ABC"
"ABC"
"ABC"
"ABC"
"st:1"
"st:2"
"st:3"
"m:4"
"XYZ"
["A","B"]
["st:5"]
call_user_func(): Argument #1 ($callback) must be a valid callback, function "no_such_fn" not found or invalid function name
call_user_func(): Argument #1 ($callback) must be a valid callback, class K does not have a method "nope"
call_user_func(): Argument #1 ($callback) must be a valid callback, function "a:b" not found or invalid function name
