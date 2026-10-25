--TEST--
A trait method may not be less visible than the inherited one it replaces
--FILE--
<?php
trait T {
    private function x() { return "T::x"; }
}
class A { public function x() { return "A::x"; } }
class B extends A { use T; }
echo "unreachable\n";
--EXPECTF--

Fatal error: Access level to B::x() must be public (as in class A) in %s on line 3
