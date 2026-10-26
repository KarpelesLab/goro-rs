--TEST--
Builtin calls: frames, compiled-special calls, zpp checks and coercions
--FILE--
<?php
namespace {
function show() {
    foreach (debug_backtrace() as $f) {
        echo "  ", $f['function'], "(", isset($f['args']) ? json_encode($f['args']) : "", ")\n";
    }
}
function cb($x) { show(); return $x; }
class C implements Countable {
    function count(): int { show(); return 3; }
    function __toString(): string { show(); return "c"; }
}
function run() {
    $c = new C;
    echo "array_map:\n";
    array_map('cb', [1]);
    echo "ARRAY_MAP:\n";
    ARRAY_MAP('cb', [1]);
    echo "count(1 arg):\n";
    echo count($c), "\n";
    echo "count(2 args):\n";
    echo count($c, COUNT_NORMAL), "\n";
    echo "sprintf call:\n";
    echo sprintf("%5s", $c), "\n";
    echo "strlen of object:\n";
    echo strlen($c), "\n";
    echo "str_repeat coerced:\n";
    echo str_repeat($c, 2), "\n";
    set_error_handler(function ($no, $msg) { echo "  error: $msg\n"; return true; });
    echo "null deprecation:\n";
    echo strlen(str_repeat("ab", null)), "\n";
    echo "float to int:\n";
    echo str_repeat("x", "2"), "\n";
    echo str_repeat("x", 2.5), "\n";
    restore_error_handler();
    echo "type errors:\n";
    $calls = [
        'str_repeat' => [[], 2], 'str_repeat ' => ["x", "y"], 'substr' => ["abc"],
        'max' => [], 'in_array' => [1, 2], 'implode' => [",", 5], 'trim' => ["x", []],
        'array_keys' => ["x"],
    ];
    foreach ($calls as $f => $args) {
        try {
            $f = trim($f);
            $f(...$args);
        } catch (Throwable $e) {
            echo get_class($e), ": ", $e->getMessage(), "\n";
            foreach ($e->getTrace() as $t) { echo "  ", $t['function'], "(", json_encode($t['args'] ?? null), ")\n"; }
        }
    }
    try { str_repeat([], 2); } catch (Throwable $e) {
        echo $e->getMessage(), "\n";
        foreach ($e->getTrace() as $t) { echo "  ", $t['function'], "(", json_encode($t['args'] ?? null), ")\n"; }
    }
    try { strlen([]); } catch (Throwable $e) {
        echo $e->getMessage(), "\n";
        foreach ($e->getTrace() as $t) { echo "  ", $t['function'], "(", json_encode($t['args'] ?? null), ")\n"; }
    }
    echo "variadic:\n";
    echo max(1, "5", 3), " ", min([4, 2]), "\n";
    try { echo json_encode(max(1, 2, [])), "\n"; } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
    echo "by-ref:\n";
    echo preg_match('/(b)/', "abc", $m), " ", json_encode($m), "\n";
    $s = "a-b"; echo str_replace("-", "+", $s, $n), " $n\n";
    echo "dynamic:\n";
    $f = 'strlen'; echo $f("abcd"), "\n";
    $f = 'StrToUpper'; echo $f("abcd"), "\n";
}
run();
}
namespace Foo {
function run2() {
    echo "namespaced:\n";
    echo json_encode(array_map('intval', ["1", "2x"])), "\n";
    echo strlen("abc"), " ", STRLEN("ab"), " ", \strlen("a"), "\n";
    try { strlen(); } catch (\Throwable $e) {
        echo $e->getMessage(), "\n";
        echo "  ", count($e->getTrace()), " frames\n";
    }
    try { str_repeat("x", -1); } catch (\Throwable $e) {
        echo $e->getMessage(), "\n";
        echo "  ", count($e->getTrace()), " frames\n";
    }
    try { STR_REPEAT([], 1); } catch (\Throwable $e) {
        echo $e->getMessage(), "\n";
        echo "  ", count($e->getTrace()), " frames\n";
    }
}
run2();
}
?>
--EXPECT--
array_map:
  show([])
  cb([1])
  array_map(["cb",[1]])
  run([])
ARRAY_MAP:
  show([])
  cb([1])
  array_map(["cb",[1]])
  run([])
count(1 arg):
  show([])
  count([])
  run([])
3
count(2 args):
  show([])
  count([])
  count([{},0])
  run([])
3
sprintf call:
  show([])
  __toString([])
  sprintf(["%5s",{}])
  run([])
    c
strlen of object:
  show([])
  __toString([])
  run([])
1
str_repeat coerced:
  show([])
  __toString([])
  str_repeat([{},2])
  run([])
cc
null deprecation:
  error: str_repeat(): Passing null to parameter #2 ($times) of type int is deprecated
0
float to int:
xx
  error: Implicit conversion from float 2.5 to int loses precision
xx
type errors:
TypeError: str_repeat(): Argument #1 ($string) must be of type string, array given
  str_repeat([[],2])
  run([])
TypeError: str_repeat(): Argument #2 ($times) must be of type int, string given
  str_repeat(["x","y"])
  run([])
ArgumentCountError: substr() expects at least 2 arguments, 1 given
  substr(["abc"])
  run([])
ArgumentCountError: max() expects at least 1 argument, 0 given
  max([])
  run([])
TypeError: in_array(): Argument #2 ($haystack) must be of type array, int given
  in_array([1,2])
  run([])
TypeError: implode(): Argument #2 ($array) must be of type ?array, int given
  implode([",",5])
  run([])
TypeError: trim(): Argument #2 ($characters) must be of type string, array given
  trim(["x",[]])
  run([])
TypeError: array_keys(): Argument #1 ($array) must be of type array, string given
  array_keys(["x"])
  run([])
str_repeat(): Argument #1 ($string) must be of type string, array given
  str_repeat([[],2])
  run([])
strlen(): Argument #1 ($string) must be of type string, array given
  run([])
variadic:
5 2
[]
by-ref:
1 ["b","b"]
a+b 1
dynamic:
4
ABCD
namespaced:
[1,2]
3 2 1
strlen() expects exactly 1 argument, 0 given
  2 frames
str_repeat(): Argument #2 ($times) must be greater than or equal to 0
  2 frames
str_repeat(): Argument #1 ($string) must be of type string, array given
  2 frames
