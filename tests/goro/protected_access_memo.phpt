--TEST--
Protected / private member access between related and unrelated classes, with classes and aliases declared later
--FILE--
<?php
class Root {
    protected $p = 'root-p';
    protected function m() { return 'Root::m'; }
    private function priv() { return 'Root::priv'; }
    public function callOn($o, $what) {
        try {
            return $what === 'm' ? $o->m() : ($what === 'p' ? $o->p : $o->priv());
        } catch (Error $e) {
            return get_class($e) . ': ' . $e->getMessage();
        }
    }
}
class Kid1 extends Root {
    protected $p = 'kid1-p';
    protected function m() { return 'Kid1::m'; }
}
class Kid2 extends Root {
    public function callOn2($o, $what) {
        try {
            return $what === 'm' ? $o->m() : ($what === 'p' ? $o->p : $o->priv());
        } catch (Error $e) {
            return get_class($e) . ': ' . $e->getMessage();
        }
    }
}
class Other {
    protected function m() { return 'Other::m'; }
    public function callOn3($o, $what) {
        try {
            return $what === 'm' ? $o->m() : ($what === 'p' ? $o->p : $o->priv());
        } catch (Error $e) {
            return get_class($e) . ': ' . $e->getMessage();
        }
    }
}

$objs = [new Root, new Kid1, new Kid2, new Other];
for ($round = 0; $round < 2; $round++) {
    foreach ($objs as $o) {
        foreach (['m', 'p', 'priv'] as $w) {
            echo get_class($o), " $w: ", (new Kid2)->callOn2($o, $w), ' | ', (new Other)->callOn3($o, $w), ' | ', (new Kid1)->callOn($o, $w), "\n";
        }
    }
    if ($round == 0) {
        eval('class Late extends Kid1 { public function callL($o) { try { return $o->m(); } catch (Error $e) { return $e->getMessage(); } } }');
        $objs[] = new Late;
        class_alias('Kid2', 'K2Alias');
        $objs[] = new K2Alias;
    }
}
var_dump((new Late)->callL(new Kid2), (new Late)->callL(new Other), (new Late)->callL(new Root));
try {
    (new Root)->m();
} catch (Error $e) {
    echo $e->getMessage(), "\n";
}
try {
    echo (new Kid1)->p;
} catch (Error $e) {
    echo $e->getMessage(), "\n";
}
?>
--EXPECTF--
Root m: Root::m | Error: Call to protected method Root::m() from scope Other | Root::m
Root p: root-p | Error: Cannot access protected property Root::$p | root-p
Root priv: Error: Call to private method Root::priv() from scope Kid2 | Error: Call to private method Root::priv() from scope Other | Root::priv
Kid1 m: Kid1::m | Error: Call to protected method Kid1::m() from scope Other | Kid1::m
Kid1 p: kid1-p | Error: Cannot access protected property Kid1::$p | kid1-p
Kid1 priv: Error: Call to private method Root::priv() from scope Kid2 | Error: Call to private method Root::priv() from scope Other | Root::priv
Kid2 m: Root::m | Error: Call to protected method Root::m() from scope Other | Root::m
Kid2 p: root-p | Error: Cannot access protected property Kid2::$p | root-p
Kid2 priv: Error: Call to private method Root::priv() from scope Kid2 | Error: Call to private method Root::priv() from scope Other | Root::priv
Other m: Error: Call to protected method Other::m() from scope Kid2 | Other::m | Error: Call to protected method Other::m() from scope Root
Other p: 
Warning: Undefined property: Other::$p in %s on line 21
 | 
Warning: Undefined property: Other::$p in %s on line 31
 | 
Warning: Undefined property: Other::$p in %s on line 8

Other priv: Error: Call to undefined method Other::priv() | Error: Call to undefined method Other::priv() | Error: Call to undefined method Other::priv()
Root m: Root::m | Error: Call to protected method Root::m() from scope Other | Root::m
Root p: root-p | Error: Cannot access protected property Root::$p | root-p
Root priv: Error: Call to private method Root::priv() from scope Kid2 | Error: Call to private method Root::priv() from scope Other | Root::priv
Kid1 m: Kid1::m | Error: Call to protected method Kid1::m() from scope Other | Kid1::m
Kid1 p: kid1-p | Error: Cannot access protected property Kid1::$p | kid1-p
Kid1 priv: Error: Call to private method Root::priv() from scope Kid2 | Error: Call to private method Root::priv() from scope Other | Root::priv
Kid2 m: Root::m | Error: Call to protected method Root::m() from scope Other | Root::m
Kid2 p: root-p | Error: Cannot access protected property Kid2::$p | root-p
Kid2 priv: Error: Call to private method Root::priv() from scope Kid2 | Error: Call to private method Root::priv() from scope Other | Root::priv
Other m: Error: Call to protected method Other::m() from scope Kid2 | Other::m | Error: Call to protected method Other::m() from scope Root
Other p: 
Warning: Undefined property: Other::$p in %s on line 21
 | 
Warning: Undefined property: Other::$p in %s on line 31
 | 
Warning: Undefined property: Other::$p in %s on line 8

Other priv: Error: Call to undefined method Other::priv() | Error: Call to undefined method Other::priv() | Error: Call to undefined method Other::priv()
Late m: Kid1::m | Error: Call to protected method Kid1::m() from scope Other | Kid1::m
Late p: kid1-p | Error: Cannot access protected property Late::$p | kid1-p
Late priv: Error: Call to private method Root::priv() from scope Kid2 | Error: Call to private method Root::priv() from scope Other | Root::priv
Kid2 m: Root::m | Error: Call to protected method Root::m() from scope Other | Root::m
Kid2 p: root-p | Error: Cannot access protected property Kid2::$p | root-p
Kid2 priv: Error: Call to private method Root::priv() from scope Kid2 | Error: Call to private method Root::priv() from scope Other | Root::priv
string(7) "Root::m"
string(51) "Call to protected method Other::m() from scope Late"
string(7) "Root::m"
Call to protected method Root::m() from global scope
Cannot access protected property Kid1::$p
