--TEST--
Property inheritance errors: readonly mismatch before the access level
--FILE--
<?php
class P { public $x = 1; }
class C extends P { protected readonly int $x; }
--EXPECTF--

Fatal error: Cannot redeclare non-readonly property P::$x as readonly C::$x in %s on line 3
