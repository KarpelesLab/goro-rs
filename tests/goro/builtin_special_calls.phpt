--TEST--
Calls of builtins php-src compiles into opcodes (is_*, count, strlen, ord, array_key_exists): results, coercions and errors
--FILE--
<?php
function show(...$v) { echo json_encode($v), "\n"; }
$vals = [null, true, false, 0, 1, -5, 1.5, NAN, "", "0", "abc", "12", [], [1, 2], new stdClass, fopen('php://memory', 'r'), function () {}, (function () { yield 1; })()];
foreach ($vals as $i => $v) {
    show($i, is_null($v), is_bool($v), is_int($v), is_integer($v), is_long($v), is_float($v), is_double($v), is_string($v), is_array($v), is_object($v), is_scalar($v));
}
$a = [1, 2, 3, 'k' => ['x' => 1]];
$r = &$a;
show(count($a), sizeof($a), count($r), count($a['k']), strlen('hello'), strlen("é"), ord('A'), ord(''), ord("\xff"));
show(array_key_exists('k', $a), array_key_exists(0, $a), array_key_exists('0', $a), array_key_exists(9, $a), array_key_exists('x', $a['k']));
// coercions / errors keep their messages
set_error_handler(function ($no, $str, $f, $l) { echo "E$no: $str (line $l)\n"; return true; });
show(strlen(12345), strlen(1.5), strlen(true));
show(strlen(null));
show(ord(65));
show(is_array($undefined1));
show(count($undefined2 ?? []));
try { count(5); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { count(new stdClass); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
class C implements Countable { function count(): int { return 42; } }
show(count(new C));
show(array_key_exists(1.5, [1 => 'a']), array_key_exists(null, ['' => 1]), array_key_exists(true, [1 => 1]));
try { array_key_exists('a', 'notarray'); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
// global scope variables bound to the symbol table
$g = [1];
show(is_array($g), count($g), is_array($GLOBALS), count($GLOBALS) > 3);
function inner() { global $g; $l = 'str'; return [is_string($l), is_array($g), count($g), strlen($l)]; }
show(inner());
// results feeding further expressions
$n = 0;
for ($i = 0; $i < 5; $i++) { if (is_int($i) && count([$i]) === 1 && strlen((string)$i) === 1) $n++; }
show($n);
// temporaries passed
show(is_array(array_merge([1], [2])), count(array_fill(0, 3, 0)), strlen(str_repeat('a', 7)), is_object(new C), is_array($a['k']), count($a['k']));
// backtrace inside a callback called from one of them (none: they do not call back)
function t() { return debug_backtrace(); }
show(count(t()));
?>
--EXPECTF--
[0,true,false,false,false,false,false,false,false,false,false,false]
[1,false,true,false,false,false,false,false,false,false,false,true]
[2,false,true,false,false,false,false,false,false,false,false,true]
[3,false,false,true,true,true,false,false,false,false,false,true]
[4,false,false,true,true,true,false,false,false,false,false,true]
[5,false,false,true,true,true,false,false,false,false,false,true]
[6,false,false,false,false,false,true,true,false,false,false,true]
[7,false,false,false,false,false,true,true,false,false,false,true]
[8,false,false,false,false,false,false,false,true,false,false,true]
[9,false,false,false,false,false,false,false,true,false,false,true]
[10,false,false,false,false,false,false,false,true,false,false,true]
[11,false,false,false,false,false,false,false,true,false,false,true]
[12,false,false,false,false,false,false,false,false,true,false,false]
[13,false,false,false,false,false,false,false,false,true,false,false]
[14,false,false,false,false,false,false,false,false,false,true,false]
[15,false,false,false,false,false,false,false,false,false,false,false]
[16,false,false,false,false,false,false,false,false,false,true,false]
[17,false,false,false,false,false,false,false,false,false,true,false]

Deprecated: ord(): Providing an empty string is deprecated in %s on line 9
[4,4,4,1,5,2,65,0,255]
[true,true,true,false,true]
[5,3,1]
E8192: strlen(): Passing null to parameter #1 ($string) of type string is deprecated (line 14)
[0]
E8192: ord(): Providing a string that is not one byte long is deprecated. Use ord($str[0]) instead (line 15)
[54]
E2: Undefined variable $undefined1 (line 16)
[false]
[0]
count(): Argument #1 ($value) must be of type Countable|array, int given
count(): Argument #1 ($value) must be of type Countable|array, stdClass given
[42]
E8192: Implicit conversion from float 1.5 to int loses precision (line 22)
E8192: Using null as the key parameter for array_key_exists() is deprecated, use an empty string instead (line 22)
[true,true,true]
array_key_exists(): Argument #2 ($array) must be of type array, string given
[true,1,true,true]
[[true,true,1,3]]
[5]
[true,3,7,true,true,1]
[1]
