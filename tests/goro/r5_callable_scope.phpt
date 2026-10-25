--TEST--
Dynamic [obj, method] calls check visibility; method closure names
--FILE--
<?php
class A {
    public function m() { return 'A::m'; }
    protected function pm() { return 'A::pm'; }
    public static function sm() {}
}
class B extends A {
    private function prm() { return 'B::prm'; }
    public function callPrm() { $f = [$this, 'prm']; return $f(); }
}
class M { private function p() { return 'p'; } public function __call($n, $a) { return "__call $n"; } }
$b = new B;
$f = [$b, 'prm'];
try { $f(); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
$f = [$b, 'pm'];
try { $f(); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
try { $b->pm(); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
var_dump($b->callPrm());
$f = [new M, 'p'];
var_dump($f());
// fromCallable deprecations name the callable form
$c = Closure::fromCallable([$b, 'parent::m']);
var_dump($c());
// a method's closure is named after the bare method
var_dump((new ReflectionFunction($b->m(...)))->getName());
var_dump((new ReflectionFunction(A::sm(...)))->getName());
var_dump((new ReflectionFunction(Closure::fromCallable([$b, 'm'])))->getName());
--EXPECTF--
Error: Call to private method B::prm() from global scope
Error: Call to protected method A::pm() from global scope
Error: Call to protected method A::pm() from global scope
string(6) "B::prm"
string(8) "__call p"

Deprecated: Callables of the form ["B", "parent::m"] are deprecated in %s on line 22
string(4) "A::m"
string(1) "m"
string(2) "sm"
string(1) "m"
