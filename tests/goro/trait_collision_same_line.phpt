--TEST--
Same-named trait methods on one line collide; one method reached through two traits does not
--FILE--
<?php
trait T { function f() { return 'T'; } }
trait U { use T; }
trait V { use T; }
class D { use U, V; }
var_dump((new D)->f());
trait A1 { function g() {} } trait A2 { function g() {} } class C { use A1, A2; }
--EXPECTF--
string(1) "T"

Fatal error: Trait method A2::g has not been applied as C::g, because of collision with A1::g in %s on line 7
