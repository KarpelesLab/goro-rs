--TEST--
Lean calls whose callees make builtin and user calls while objects with destructors exist: destructors run at the same points, with the same frames
--FILE--
<?php
class D {
    public $n;
    function __construct($n) { $this->n = $n; }
    function __destruct() {
        echo "dtor {$this->n}:";
        foreach (debug_backtrace() as $f) {
            echo " ", $f['function'], "@", $f['line'] ?? '-';
        }
        echo "\n";
    }
}
$keep = new D('keep');
function inner($x) { return strlen($x); }
function lean1($a) {
    $r = count([new D("arg$a")]);
    $s = inner('abc');
    return $r + $s;
}
function lean2($o) {
    $x = strtoupper('a');
    return $x;
}
for ($i = 0; $i < 3; $i++) {
    echo lean1($i), "\n";
}
echo lean2(new D('passed')), "\n";
function lean3() {
    $o = new D('local');
    $n = strlen('xy');
    return $n;
}
echo lean3(), "\n";
function mk($n) { return new D($n); }
function lean4($v) {
    $n = strlen(mk("res$v")->n);
    $m = max(1, 2);
    return $n + $m;
}
echo lean4(7), "\n";
class T {
    function __destruct() { throw new Exception("from dtor"); }
}
function lean6() { return count([new T]); }
function lean7() { return strlen(str_repeat('a', 2)) + lean6(); }
for ($i = 0; $i < 2; $i++) {
    try {
        echo lean7(), "\n";
    } catch (Exception $e) {
        echo get_class($e), ": ", $e->getMessage(), " line ", $e->getLine(), "\n";
        foreach ($e->getTrace() as $f) {
            echo "  ", $f['function'], "@", $f['line'] ?? '-', "\n";
        }
    }
}
function user_frame($o) { return $o->n; }
function lean8() {
    $s = user_frame(new D('uf'));
    $t = strlen($s);
    return $t;
}
echo lean8(), "\n";
function lean9($a, $b) {
    $x = implode(',', [$a, $b]);
    $y = in_array($a, [1, 2, 3], true);
    return $x . ($y ? 'y' : 'n');
}
for ($i = 0; $i < 3; $i++) {
    echo lean9($i, new D("b$i") ? 'o' : 'x'), "\n";
}
echo "end\n";
--EXPECT--
dtor arg0: __destruct@16 lean1@25
4
dtor arg1: __destruct@16 lean1@25
4
dtor arg2: __destruct@16 lean1@25
4
dtor passed: __destruct@27
A
dtor local: __destruct@33
2
dtor res7: __destruct@36 lean4@40
6
Exception: from dtor line 42
  __destruct@44
  lean6@45
  lean7@48
Exception: from dtor line 42
  __destruct@44
  lean6@45
  lean7@48
dtor uf: __destruct@58 lean8@62
2
dtor b0: __destruct@69
0,on
dtor b1: __destruct@69
1,oy
dtor b2: __destruct@69
2,oy
end
dtor keep: __destruct@-
