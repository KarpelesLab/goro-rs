--TEST--
new of a class without a constructor skips the constructor call
--FILE--
<?php
class Plain { public $x = 1; }
class WithDtor { public $n; function __destruct() { echo "dtor\n"; } }
class Base { public $made = false; function __construct() { $this->made = true; } }
class Child extends Base { }
class GrandChild extends Child { public $g = 'g'; }
function make() { return new Plain; }
for ($i = 0; $i < 3; $i++) {
    $p = new Plain;
    $p->x += $i;
    $c = new Child;
    $g = new GrandChild;
    echo $p->x, ' ', var_export($c->made, true), ' ', var_export($g->made, true), "\n";
}
$objs = [new Plain, new Plain, make()];
echo count($objs), ' ', spl_object_id($objs[0]) != spl_object_id($objs[1]) ? 'distinct' : 'same', "\n";
echo (new Plain)->x, "\n";
var_dump(new Plain == new Plain);
$w = new WithDtor;
echo "made\n";
unset($w);
echo "after unset\n";
new WithDtor;
echo "after discard\n";
function args() { echo "arg evaluated\n"; return 1; }
$q = new Plain(args());
echo get_class($q), "\n";
$anon = new class { public $a = 'anon'; };
echo $anon->a, "\n";
for ($i = 0; $i < 2; $i++) { $anon2 = new class { public $b = 2; }; echo $anon2->b; }
echo "\n";
$cls = 'Plain';
$d = new $cls;
echo $d->x, "\n";
set_error_handler(function ($no, $str) { echo "handler: $str\n"; return true; });
$e = new Plain;
echo $e->undefined ?? 'none', "\n";
try { $f = new Plain; throw new Exception('x'); } catch (Exception $ex) { echo get_class($f), "\n"; }
class Counter { public static $n = 0; public $id; }
$list = [];
for ($i = 0; $i < 4; $i++) { $list[] = new Counter; }
echo count($list), "\n";
?>
--EXPECT--
1 true true
2 true true
3 true true
3 distinct
1
bool(true)
made
dtor
after unset
dtor
after discard
arg evaluated
Plain
anon
22
1
none
Plain
4
