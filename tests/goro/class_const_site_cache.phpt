--TEST--
Class constants by self::, parent:: and static::, inherited and overridden
--FILE--
<?php
interface IF1 { const IC = 'ic'; }
class A implements IF1 { const X = 1; const ARR = [1, 2]; protected const P = 'p'; private const Q = 'qa'; static function q() { return self::Q; } }
class B extends A { const Y = self::X + 1; private const Q = 'qb'; function g() { return [self::X, static::X, parent::X, self::P, parent::P, self::ARR, self::IC, parent::q()]; } static function h() { return static::Y; } }
class C extends B { const X = 10; }
for ($i = 0; $i < 3; $i++) {
    echo json_encode([A::X, B::X, C::X, B::Y, C::Y, (new B)->g(), (new C)->g(), B::h(), C::h(), C::IC, A::q()]), "\n";
}
try { echo C::NOPE; } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
[1,1,10,2,2,[1,1,1,"p","p",[1,2],"ic","qa"],[1,10,1,"p","p",[1,2],"ic","qa"],2,2,"ic","qa"]
[1,1,10,2,2,[1,1,1,"p","p",[1,2],"ic","qa"],[1,10,1,"p","p",[1,2],"ic","qa"],2,2,"ic","qa"]
[1,1,10,2,2,[1,1,1,"p","p",[1,2],"ic","qa"],[1,10,1,"p","p",[1,2],"ic","qa"],2,2,"ic","qa"]
Undefined constant C::NOPE
