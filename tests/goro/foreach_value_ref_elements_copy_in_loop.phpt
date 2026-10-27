--TEST--
By-value foreach over an array whose elements are references only it holds: a copy of the array made in the loop gets plain values (the copies do not share the elements)
--FILE--
<?php
class Ctx {
    public array $v = [];
    function touch(): void { foreach ($this->v as $k => &$t) { if ($k === 'zz') {} } }
}
final class A {
    static function comb($a, $b) { return $a . '+' . $b; }
    static function run(Ctx $c, Ctx $orig): void {
        $inner = clone $c;
        foreach ($c->v as $k => $t) {
            if (isset($orig->v[$k]) && $t !== $orig->v[$k]) {
                $c->v[$k] = self::comb($c->v[$k], $orig->v[$k]);
            }
        }
        var_dump($inner->v);
        $c2 = clone $c;
        $c2->v['a'] = 'C2';
        var_dump($c->v['a'], $c2->v['a'], $inner->v['a']);
    }
}
function main() {
    $orig = new Ctx; $orig->v = ['a' => 'o', 'b' => 'o'];
    $c = new Ctx; $c->v = ['a' => 'x', 'b' => 'y'];
    $c->touch();
    A::run($c, $orig);
}
main();

// a plain array
$x = ['a' => 1, 'b' => 2];
foreach ($x as &$r) {} unset($r);
foreach ($x as $k => $t) { $y = $x; $y[$k] = 'Y'; }
var_dump($x);

// references shared with a variable still show what the loop body does to it
$b = 2;
$arr = [1, &$b, 3];
foreach ($arr as $v) { $b = 99; echo $v, "\n"; }
// an element that is a reference to an object
$o = new stdClass; $o->p = 1;
$z = [$o, 2];
foreach ($z as &$q) {} unset($q);
foreach ($z as $k => $t) { $w = $z; $w[0] = 'W'; }
var_dump($z[0]->p, $w[0]);
?>
--EXPECT--
array(2) {
  ["a"]=>
  string(1) "x"
  ["b"]=>
  string(1) "y"
}
string(3) "x+o"
string(2) "C2"
string(1) "x"
array(2) {
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
}
1
99
3
int(1)
string(1) "W"
