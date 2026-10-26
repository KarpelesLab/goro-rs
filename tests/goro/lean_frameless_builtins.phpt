--TEST--
Builtins called by lean callees without their frames: what they raise and the user code they run see the frames
--FILE--
<?php
// Builtins called by lean callees: what they raise, and the user code they
// run, see the callees' frames.
function bt() {
    $o = [];
    foreach (debug_backtrace() as $f) {
        $o[] = ($f['class'] ?? '') . ($f['type'] ?? '') . $f['function'] . '@' . ($f['line'] ?? '-') . '(' . count($f['args'] ?? []) . ')';
    }
    return implode(' < ', $o);
}
class S {
    public $v;
    function __construct($v) { $this->v = $v; }
    function __toString(): string { echo "  toString: ", bt(), "\n"; return (string)$this->v; }
}
class J implements JsonSerializable {
    function jsonSerialize(): mixed { echo "  json: ", bt(), "\n"; return [1, 2]; }
}
class D {
    public $n;
    function __construct($n) { $this->n = $n; }
    function __destruct() { echo "  destruct {$this->n}: ", bt(), "\n"; }
}
function inner_implode($a) { return implode(',', $a); }
function outer_implode($a) { return inner_implode($a) . '!'; }
function do_strrepeat($n) { return str_repeat('x', $n); }
function do_strlen($v) { return strlen($v); }
function do_json($v) { return json_encode($v); }
function do_replace($s) { return str_replace('a', new S('b'), $s); }
function do_pop(&$a) { return array_pop($a); }
function do_sprintf($f, $v) { return sprintf($f, $v); }
function do_substr($s, $n) { return substr($s, $n); }
function do_explode($d, $s) { return explode($d, $s); }

for ($i = 0; $i < 3; $i++) {
    echo outer_implode([1, new S("s$i"), 3]), "\n";
}
try {
    echo do_strrepeat(-1), "\n";
} catch (ValueError $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
    foreach ($e->getTrace() as $f) echo "  ", $f['function'], "@", $f['line'] ?? '-', "\n";
}
try {
    echo do_strlen([]), "\n";
} catch (TypeError $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
    foreach ($e->getTrace() as $f) echo "  ", $f['function'], "@", $f['line'] ?? '-', "\n";
}
try {
    echo do_explode('', 'abc'), "\n";
} catch (ValueError $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
    foreach ($e->getTrace() as $f) echo "  ", $f['function'], "@", $f['line'] ?? '-', "\n";
}
echo do_json(new J), "\n";
echo do_replace("banana"), "\n";
$arr = [new D(1), new D(2)];
echo get_class(do_pop($arr)), "\n";
echo "after pop\n";
set_error_handler(function ($no, $str, $file, $line) {
    echo "  handler: $str @$line: ", bt(), "\n";
    return true;
});
echo do_sprintf('%d', []), "\n";
echo do_substr([], 1) ?? 'null', "\n";
function warn_inner($s) { return htmlspecialchars($s, ENT_QUOTES, 'no-such-charset'); }
function warn_outer($s) { return warn_inner($s); }
echo warn_outer("<a>"), "\n";
restore_error_handler();
echo warn_outer("<b>"), "\n";
function thrower() {
    set_error_handler(function () { throw new Exception("from handler"); });
    try {
        return htmlspecialchars("x", ENT_QUOTES, 'nope');
    } finally {
        restore_error_handler();
    }
}
function call_thrower() { return thrower(); }
try {
    call_thrower();
} catch (Exception $e) {
    echo "caught ", $e->getMessage(), "\n";
    foreach ($e->getTrace() as $f) echo "  ", $f['function'], "@", $f['line'] ?? '-', "\n";
}
class T {
    function m($a) { return array_keys($a); }
    function n($a) { return count($this->m($a)) . ':' . implode('-', $this->m($a)); }
}
$t = new T;
for ($i = 0; $i < 3; $i++) echo $t->n(['a' => 1, 'b' => 2, $i => 3]), "\n";
function deep($n) { return $n ? deep($n - 1) : implode(',', [new S('deep')]); }
echo deep(3), "\n";
echo "done\n";
--EXPECTF--
  toString: bt@14(0) < S->__toString@-(0) < implode@24(2) < inner_implode@25(1) < outer_implode@36(1)
1,s0,3!
  toString: bt@14(0) < S->__toString@-(0) < implode@24(2) < inner_implode@25(1) < outer_implode@36(1)
1,s1,3!
  toString: bt@14(0) < S->__toString@-(0) < implode@24(2) < inner_implode@25(1) < outer_implode@36(1)
1,s2,3!
ValueError: str_repeat(): Argument #2 ($times) must be greater than or equal to 0
  str_repeat@26
  do_strrepeat@39
TypeError: strlen(): Argument #1 ($string) must be of type string, array given
  do_strlen@45
ValueError: explode(): Argument #1 ($separator) must not be empty
  explode@33
  do_explode@51
  json: bt@17(0) < J->jsonSerialize@-(0) < json_encode@28(1) < do_json@56(1)
[1,2]
  toString: bt@14(0) < S->__toString@-(0) < str_replace@29(3) < do_replace@57(1)
bbnbnb
  destruct 2: bt@22(0) < D->__destruct@59(0)
D
after pop
0

Fatal error: Uncaught TypeError: substr(): Argument #1 ($string) must be of type string, array given in %s:32
Stack trace:
#0 %s(32): substr(Array, 1)
#1 %s(66): do_substr(Array, 1)
#2 {main}
  thrown in %s on line 32
  destruct 1: bt@22(0) < D->__destruct@-(0)
