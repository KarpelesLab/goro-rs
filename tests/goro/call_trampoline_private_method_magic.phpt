--TEST--
A call trampoline for an inaccessible method calls __call() or __callStatic() in its own frame; its ReflectionFunction is internal
--FILE--
<?php
class A {
  public function __call($n, $a) { echo "__call $n ", static::class, " ", json_encode($a), "\n"; debug_print_backtrace(); return 42; }
  public static function __callStatic($n, $a) { echo "__callStatic $n ", static::class, " ", json_encode($a), "\n"; debug_print_backtrace(); return 43; }
  private function priv() { echo "priv\n"; }
  private static function spriv() { echo "spriv\n"; }
  protected function prot() { echo "prot\n"; }
}
class B extends A { function viaB() { return $this->prot(...); } function t2() { parent::priv(4); } }
$a = new A;
var_dump(($a->priv(...))(1, 2));
var_dump(($a->undefined(...))(3));
var_dump((A::spriv(...))(4));
var_dump((Closure::fromCallable([$a, 'priv']))(5));
A::spriv(6);
((new B)->viaB())();
(new B)->t2();
foreach ([$a->priv(...), A::stat(...), Closure::fromCallable([$a, 'priv'])] as $c) {
  $r = new ReflectionFunction($c);
  var_dump($r->getName(), $r->isInternal(), $r->isUserDefined(), $r->getNumberOfParameters(), $r->getFileName());
  echo $r;
}
class T { function __call($n, $a) { throw new Exception("in $n"); } private function p() {} }
function thrower($c) { $c(); }
try { thrower((new T)->p(...)); } catch (Exception $ex) { echo $ex->getTraceAsString(), "\n"; }
--EXPECTF--
__call priv A [1,2]
#0 [internal function]: A->__call('priv', Array)
#1 %s: A->priv(1, 2)
int(42)
__call undefined A [3]
#0 [internal function]: A->__call('undefined', Array)
#1 %s: A->undefined(3)
int(42)
__callStatic spriv A [4]
#0 [internal function]: A::__callStatic('spriv', Array)
#1 %s: A::spriv(4)
int(43)
__call priv A [5]
#0 [internal function]: A->__call('priv', Array)
#1 %s: A->priv(5)
int(42)
__callStatic spriv A [6]
#0 %s: A::__callStatic('spriv', Array)
prot
__call priv B [4]
#0 %s: A->__call('priv', Array)
#1 %s: B->t2()
string(4) "priv"
bool(true)
bool(false)
int(1)
bool(false)
Closure [ <internal> public method priv ] {

  - Parameters [1] {
    Parameter #0 [ <optional> mixed ...$arguments ]
  }
}
string(4) "stat"
bool(true)
bool(false)
int(1)
bool(false)
Closure [ <internal> static public method stat ] {

  - Parameters [1] {
    Parameter #0 [ <optional> mixed ...$arguments ]
  }
}
string(4) "priv"
bool(true)
bool(false)
int(0)
bool(false)
Closure [ <internal> public method priv ] {
}
#0 [internal function]: T->__call('p', Array)
#1 %s: T->p()
#2 %s: thrower(Object(Closure))
#3 {main}
