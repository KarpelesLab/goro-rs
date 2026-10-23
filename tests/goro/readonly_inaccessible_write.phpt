--TEST--
Writing an inaccessible readonly property reports its visibility first
--FILE--
<?php
class A { protected readonly array $x; public function __construct() { $this->x = ["a" => 1]; } public function __isset($n) { return $n === "x"; } }
$o = new A;
try { $o->x = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $o->x ??= 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $o->x .= "z"; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { unset($o->x); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $o->x[] = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $o->x++; } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
Cannot access protected property A::$x
Cannot access protected property A::$x
Cannot access protected property A::$x
Cannot access protected property A::$x
Cannot access protected property A::$x
Cannot access protected property A::$x
