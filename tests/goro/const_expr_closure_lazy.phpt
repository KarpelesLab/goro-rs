--TEST--
Closures in class constants and property defaults are created when the class's constants are updated, not at declaration
--FILE--
<?php
trait T { public $tp = static function() { return "tp"; }; public static $ts = static function() { return "ts"; }; }
class P { const PC = static function() { return "pc"; }; }
class C extends P {
    use T;
    const G = static function() { return "g"; };
    public static $s = static function() { return "s"; };
    public $p = static function() { return "p"; };
}
var_dump(spl_object_id(new stdClass));
var_dump(spl_object_id(C::$s));
var_dump(spl_object_id(C::$ts));
var_dump(spl_object_id(C::G));
var_dump(spl_object_id(C::PC), C::PC === P::PC);
$c = new C;
$c2 = new C;
var_dump(spl_object_id($c->p), spl_object_id($c->tp), $c->p === $c2->p);
var_dump(spl_object_id(new stdClass));
echo (C::G)(), (C::$s)(), (C::$ts)(), (C::PC)(), ($c->p)(), ($c->tp)(), "\n";
class D { public $f = strlen(...); const K = strlen(...); }
var_dump(spl_object_id(new stdClass));
$d = new D; $e = new D;
var_dump($d->f === $e->f, D::K === D::K, (D::K)("abc"));
--EXPECT--
int(1)
int(5)
int(6)
int(2)
int(1)
bool(true)
int(3)
int(4)
bool(true)
int(9)
gstspcptp
int(9)
bool(true)
bool(true)
int(3)
