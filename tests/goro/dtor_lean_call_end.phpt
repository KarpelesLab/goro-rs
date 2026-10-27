--TEST--
Calls made while objects with destructors exist: destructors due at the end of the call run there
--FILE--
<?php
class Keep { function __destruct() { echo "keep gone\n"; } }
class D {
    function __construct(public string $n, public bool $throw = false) {}
    function __destruct() {
        echo "D {$this->n} gone\n";
        if ($this->throw) {
            throw new Exception("from {$this->n}");
        }
    }
}
class Wrap {
    public $d;
    function __construct($d) { $this->d = $d; }
    function m(int $x): int { return $x + 1; }
    function n() { return 2; }
}
class T {
    public int $c = 0;
    function inc(): int { $this->c++; return $this->c; }
    function pass($v) { return $v; }
    function drop($v) { $v = null; return 1; }
}
function plain($a, $b) { $c = $a + $b; return $c; }
function main() {
$t = new T;
$s = 0;
for ($i = 0; $i < 3000; $i++) {
    $s += plain($i, 1) + $t->inc();
}
echo $s, "\n";
for ($r = 0; $r < 3; $r++) {
    echo (new Wrap(new D("w$r")))->m($r), "\n";
    echo "after w$r\n";
    $x = $t->pass(new D("p$r"));
    echo "kept p$r\n";
    $x = null;
    echo "after p$r\n";
    $w = new Wrap(new D("q$r"));
    echo $t->drop($w), "\n";
    echo "still q$r\n";
    $w = null;
    echo "after q$r\n";
}
try {
    echo (new Wrap(new D("e", true)))->n(), "\n";
    echo "not here\n";
} catch (Exception $e) {
    echo "caught ", $e->getMessage(), "\n";
}
echo deep($t, 5), "\n";
}
function deep(T $t, $n) { return $n == 0 ? $t->inc() : deep($t, $n - 1) + 1; }
$keep = new Keep;
main();
echo "end\n";
?>
--EXPECT--
9003000
D w0 gone
1
after w0
kept p0
D p0 gone
after p0
1
still q0
D q0 gone
after q0
D w1 gone
2
after w1
kept p1
D p1 gone
after p1
1
still q1
D q1 gone
after q1
D w2 gone
3
after w2
kept p2
D p2 gone
after p2
1
still q2
D q2 gone
after q2
D e gone
caught from e
3006
end
keep gone
