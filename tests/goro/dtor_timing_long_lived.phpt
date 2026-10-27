--TEST--
Destructors run at once while long-lived objects with destructors exist (plain-value opcodes in between)
--FILE--
<?php
class Keep { function __destruct() { echo "keep gone\n"; } }
class D {
    function __construct(public string $n) {}
    function __destruct() { echo "D {$this->n} gone\n"; }
}
function make(string $n): D { return new D($n); }
function work(int $n): int {
    $s = 0;
    for ($i = 0; $i < $n; $i++) {
        $x = $i % 7;
        if ($x == 3) { $s += $x; } else { $s -= 1; }
        $t = "s";
        $u = $t;
    }
    return $s;
}
$keep = [];
for ($i = 0; $i < 100; $i++) {
    $keep[] = new Keep;
}
// (many release points: the long-lived ones age)
echo work(5000), "\n";
for ($r = 0; $r < 3; $r++) {
    $o = new D("a$r");
    $o = 1;
    echo "after a$r\n";
    $o = new D("b$r");
    $p = $o;
    $o = 2;
    echo "still b$r\n";
    $p = "x";
    echo "after b$r\n";
    make("c$r");
    echo "after c$r\n";
    $arr = [new D("d$r"), 1];
    $arr = 3;
    echo "after d$r\n";
    $s = work(200);
    $q = new D("e$r");
    $s = work(10) + strlen("abc");
    unset($q);
    echo "after e$r $s\n";
}
function scoped() {
    $l = new D("local");
    $z = 0;
    for ($i = 0; $i < 10; $i++) { $z += $i; }
    return $z;
}
echo scoped(), "\n";
echo "end\n";
$keep = null;
echo "done\n";
?>
--EXPECT--
-2144
D a0 gone
after a0
still b0
D b0 gone
after b0
D c0 gone
after c0
D d0 gone
after d0
D e0 gone
after e0 -3
D a1 gone
after a1
still b1
D b1 gone
after b1
D c1 gone
after c1
D d1 gone
after d1
D e1 gone
after e1 -3
D a2 gone
after a2
still b2
D b2 gone
after b2
D c2 gone
after c2
D d2 gone
after d2
D e2 gone
after e2 -3
D local gone
45
end
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
keep gone
done
