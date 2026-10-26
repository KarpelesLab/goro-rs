--TEST--
Static and forwarding method calls without a frame of their own get one when their callee needs it (builtin and user calls): backtraces, self/static/parent, recursion, error handlers, exceptions
--FILE--
<?php
function frames() {
    return implode(' < ', array_map(fn($f) => ($f['class'] ?? '') . ($f['type'] ?? '') . $f['function'] . '(' . json_encode($f['args'] ?? []) . ')', debug_backtrace()));
}
class Base {
    static function sb($x) { return strlen($x) . ':' . frames(); }
    function ib($x) { return strtoupper($x) . ':' . frames(); }
}
class T extends Base {
    static function conv($d) { return strtoupper($d); }
    static function r($tree) {
        foreach ($tree as $k => $d) {
            if (is_string($d) && str_starts_with($d, 'v')) { $tree[$k] = self::conv($d); }
            elseif (is_array($d)) { $tree[$k] = self::r($d); }
        }
        return $tree;
    }
    static function bt($a) { return frames(); }
    static function viaself($a) { return self::bt($a + 1); }
    static function viastatic($a) { return static::bt($a + 2); }
    static function viaparent($a) { return parent::sb("p$a"); }
    function inst($a) { return self::bt($a) . ' | ' . static::sb("s$a") . ' | ' . parent::ib("i$a"); }
    static function warn($a) { return $a + $undefinedvar; }
    static function thrower($a) { return intdiv($a, 0); }
    static function deep($n) { return $n <= 0 ? frames() : self::deep($n - 1); }
}
class U extends T {
    static function bt($a) { return 'U:' . frames(); }
}
function run() {
    $t = ['a' => 'v1', 'b' => ['c' => 'v2', 'd' => ['e' => 'x']]];
    for ($i = 0; $i < 3; $i++) { $x = T::r($t); }
    echo json_encode($x), "\n";
    for ($i = 0; $i < 2; $i++) {
        echo T::bt($i), "\n";
        echo T::viaself($i), "\n";
        echo T::viastatic($i), "\n";
        echo U::viastatic($i), "\n";
        echo T::viaparent($i), "\n";
        echo (new T)->inst($i), "\n";
        echo (new U)->inst($i), "\n";
        echo T::deep(2), "\n";
    }
    set_error_handler('handler');
    for ($i = 0; $i < 2; $i++) { echo T::warn($i), "\n"; }
    restore_error_handler();
    for ($i = 0; $i < 2; $i++) {
        try { T::thrower($i); } catch (DivisionByZeroError $e) {
            echo $e->getMessage(), "\n";
            foreach ($e->getTrace() as $f) { echo "  ", $f['class'] ?? '', $f['type'] ?? '', $f['function'], "\n"; }
        }
    }
}
function handler($no, $str, $file, $line) { echo "handler: $str line $line ", implode(' < ', array_map(fn($f) => ($f['class'] ?? '') . ($f['type'] ?? '') . $f['function'], debug_backtrace())), "\n"; return true; }
run();
--EXPECT--
{"a":"V1","b":{"c":"V2","d":{"e":"x"}}}
frames([]) < T::bt([0]) < run([])
frames([]) < T::bt([1]) < T::viaself([0]) < run([])
frames([]) < T::bt([2]) < T::viastatic([0]) < run([])
U:frames([]) < U::bt([2]) < T::viastatic([0]) < run([])
2:frames([]) < Base::sb(["p0"]) < T::viaparent([0]) < run([])
frames([]) < T::bt([0]) < T->inst([0]) < run([]) | 2:frames([]) < Base::sb(["s0"]) < T->inst([0]) < run([]) | I0:frames([]) < Base->ib(["i0"]) < T->inst([0]) < run([])
frames([]) < T::bt([0]) < T->inst([0]) < run([]) | 2:frames([]) < Base::sb(["s0"]) < T->inst([0]) < run([]) | I0:frames([]) < Base->ib(["i0"]) < T->inst([0]) < run([])
frames([]) < T::deep([0]) < T::deep([1]) < T::deep([2]) < run([])
frames([]) < T::bt([1]) < run([])
frames([]) < T::bt([2]) < T::viaself([1]) < run([])
frames([]) < T::bt([3]) < T::viastatic([1]) < run([])
U:frames([]) < U::bt([3]) < T::viastatic([1]) < run([])
2:frames([]) < Base::sb(["p1"]) < T::viaparent([1]) < run([])
frames([]) < T::bt([1]) < T->inst([1]) < run([]) | 2:frames([]) < Base::sb(["s1"]) < T->inst([1]) < run([]) | I1:frames([]) < Base->ib(["i1"]) < T->inst([1]) < run([])
frames([]) < T::bt([1]) < T->inst([1]) < run([]) | 2:frames([]) < Base::sb(["s1"]) < T->inst([1]) < run([]) | I1:frames([]) < Base->ib(["i1"]) < T->inst([1]) < run([])
frames([]) < T::deep([0]) < T::deep([1]) < T::deep([2]) < run([])
handler: Undefined variable $undefinedvar line 23 handler < T::warn < run
0
handler: Undefined variable $undefinedvar line 23 handler < T::warn < run
1
Division by zero
  intdiv
  T::thrower
  run
Division by zero
  intdiv
  T::thrower
  run
