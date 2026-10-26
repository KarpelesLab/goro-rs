--TEST--
Builtin calls under strict_types: zpp checks of plain, referenced and variadic arguments
--FILE--
<?php
declare(strict_types=1);
function t(callable $f) {
    try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
function run() {
    $s = "abc"; $r = &$s; $i = 5; $ri = &$i; $a = [3, 1, 2]; $ra = &$a;
    t(fn() => strlen($s));
    t(fn() => strlen($r));
    t(fn() => strlen($i));
    t(fn() => strlen($ri));
    t(fn() => str_repeat($s, $i));
    t(fn() => str_repeat($r, $ri));
    t(fn() => str_repeat($s, "2"));
    t(fn() => str_repeat($s, 2.0));
    t(fn() => sqrt($i));
    t(fn() => round($ri, 1));
    t(fn() => in_array(1, $ra));
    t(fn() => in_array(1, $ra, 1));
    t(fn() => implode(",", $ra));
    t(fn() => max($i, 7, 2.5));
    t(fn() => array_keys($ra, 1, true));
    t(fn() => array_keys($ra, 1, "yes"));
    t(fn() => trim($r, null));
    t(fn() => substr($s, $i, null));
    t(fn() => substr($s, "1"));
    t(fn() => array_merge($a, $ra, [4]));
    t(fn() => array_merge($a, 5));
    t(fn() => str_contains($r, "b"));
    t(fn() => strpos($r, "c", $ri - 5));
    t(fn() => strpos($r, "c", 1.0));
    t(fn() => preg_match('/b/', $r));
    t(fn() => preg_match('/b/', $ri));
    t(fn() => preg_replace('/b/', 'x', $r));
    t(fn() => count($ra));
    t(fn() => count($s));
    t(fn() => is_array($ra));
    t(fn() => array_key_exists(1, $ra));
    for ($k = 0; $k < 3; $k++) {
        t(fn() => str_repeat("ab", $k));
        t(fn() => strlen($k ? $s : $i));
    }
}
run();
?>
--EXPECT--
int(3)
int(3)
TypeError: strlen(): Argument #1 ($string) must be of type string, int given
TypeError: strlen(): Argument #1 ($string) must be of type string, int given
string(15) "abcabcabcabcabc"
string(15) "abcabcabcabcabc"
TypeError: str_repeat(): Argument #2 ($times) must be of type int, string given
TypeError: str_repeat(): Argument #2 ($times) must be of type int, float given
float(2.23606797749979)
float(5)
bool(true)
TypeError: in_array(): Argument #3 ($strict) must be of type bool, int given
string(5) "3,1,2"
int(7)
array(1) {
  [0]=>
  int(1)
}
TypeError: array_keys(): Argument #3 ($strict) must be of type bool, string given
TypeError: trim(): Argument #2 ($characters) must be of type string, null given
string(0) ""
TypeError: substr(): Argument #2 ($offset) must be of type int, string given
array(7) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [2]=>
  int(2)
  [3]=>
  int(3)
  [4]=>
  int(1)
  [5]=>
  int(2)
  [6]=>
  int(4)
}
TypeError: array_merge(): Argument #2 must be of type array, int given
bool(true)
int(2)
TypeError: strpos(): Argument #3 ($offset) must be of type int, float given
int(1)
TypeError: preg_match(): Argument #2 ($subject) must be of type string, int given
string(3) "axc"
int(3)
TypeError: count(): Argument #1 ($value) must be of type Countable|array, string given
bool(true)
bool(true)
string(0) ""
TypeError: strlen(): Argument #1 ($string) must be of type string, int given
string(2) "ab"
int(3)
string(4) "abab"
int(3)
