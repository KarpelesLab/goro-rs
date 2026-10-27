--TEST--
The assignment of a promoted property is on the parameter's line
--FILE--
<?php
class A {
    public function __construct(
        public int $x { get => 1; },
    ) {}
}
try { new A(5); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
class B {
    public function __construct(
        public readonly int $x,
    ) {
        $this->x = 2;
    }
}
try { new B(5); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
class C {
    public function __construct(
        public int $x {
            set {
                throw new Exception("in set");
            }
        },
    ) {}
}
try { new C(5); } catch (Exception $e) { echo $e->getMessage(), " @", $e->getLine(), "\n", $e->getTraceAsString(), "\n"; }
class D {
    public function __construct(
        public int $x { set => $value; },
    ) {}
}
var_dump(new D(3));
class F { public function __construct(public private(set) array $a = []) {} }
$f = new F([1]);
try { $f->a[] = 2; } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
class G {
    public function __construct(
        public $x {
            get => throw new Exception("get");
        }
    ) {}
}
try { new G(1); } catch (Throwable $e) { echo get_class($e), $e->getMessage(), " @", $e->getLine(), "\n"; }
--EXPECTF--
Property A::$x is read-only @4
Cannot modify readonly property B::$x @12
in set @20
#0 %s(18): C->$x::set(5)
#1 %s(25): C->__construct(5)
#2 {main}
object(D)#3 (1) {
  ["x"]=>
  int(3)
}
Cannot indirectly modify private(set) property F::$a from global scope @34
ErrorProperty G::$x is read-only @37
