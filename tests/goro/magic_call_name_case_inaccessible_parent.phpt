--TEST--
__call() of an inaccessible parent:: method gets the name as the call spelled it
--FILE--
<?php
class A {
    public function __call($n, $a) { echo "__call($n)\n"; }
    public static function __callStatic($n, $a) { echo "__callStatic($n)\n"; }
    private function callPrivateParent() {}
    private static function privStatic() {}
}
class B extends A {}
class C extends B {
    public function callPrivateParent() { parent::callPrivateParent(); }
    public function viaStatic() { parent::privStatic(); }
    public function missing() { parent::missingMethod(); }
    public static function smissing() { parent::missingStatic(); }
}
$c = new C;
$c->callPrivateParent();
$c->viaStatic();
$c->missing();
C::smissing();

?>
--EXPECT--
__call(callPrivateParent)
__call(privStatic)
__call(missingMethod)
__callStatic(missingStatic)
