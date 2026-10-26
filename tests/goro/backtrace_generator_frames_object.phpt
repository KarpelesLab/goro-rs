--TEST--
debug_backtrace() of generator frames: object of delegating generators, class of closure generators
--FILE--
<?php
function show() {
    foreach (debug_backtrace(DEBUG_BACKTRACE_PROVIDE_OBJECT | DEBUG_BACKTRACE_IGNORE_ARGS) as $i => $f) {
        echo $i, ' ', $f['class'] ?? '', $f['type'] ?? '', $f['function'], ' :', str_contains($f['function'], '{closure') ? '?' : ($f['line'] ?? '-'), ' ', isset($f['object']) ? get_class($f['object']) : '-', "\n";
    }
}
class T {
    public $p = 1;
    function m() { show(); }
    function g() {
        $f = function () {
            show();
        };
        $f();
        yield 1;
    }
    function y() {
        yield from $this->g();
    }
    function c() {
        yield from (function () {
            show();
            yield $this->p;
        })();
    }
    function s() {
        yield from (static function () {
            show();
            yield 2;
        })();
    }
}
(new T)->m();
echo "--\n";
foreach ((new T)->y() as $_) {}
echo "--\n";
foreach ((new T)->c() as $_) {}
echo "--\n";
foreach ((new T)->s() as $_) {}

?>
--EXPECT--
0 show :9 -
1 T->m :33 T
--
0 show :12 -
1 T->{closure:T::g():11} :? T
2 T->g :18 T
3 T->y :35 T
--
0 show :22 -
1 T->{closure:T::c():21} :? T
2 T->c :37 T
--
0 show :28 -
1 T::{closure:T::s():27} :? -
2 T->s :39 T
