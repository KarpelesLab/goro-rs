--TEST--
Property inheritance errors: each parent property in turn
--FILE--
<?php
class P { public int $a = 0; protected $b = 1; }
class C extends P { public string $a = ''; protected readonly int $b; }
--EXPECTF--

Fatal error: Type of C::$a must be int (as in class P) in %s on line 3
