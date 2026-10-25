--TEST--
Property hooks: an inaccessible hooked property is not hooked from outside its scope
--FILE--
<?php
class A { private $x = 1 { get => $this->x + 1; set => $value; } protected $v { get => 5; }
  function t() { var_dump($this->x, $this->v); $this->x = 10; var_dump($this->x); } }
$a = new A;
$a->t();
try { var_dump($a->x); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { var_dump($a->v); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $a->x = 3; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { var_dump(isset($a->x)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { var_dump(isset($a->v)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $a->v = 3; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $a->x .= 3; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $a->x++; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { unset($a->x); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $r = &$a->x; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $a->x[] = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
class B extends A { function u() { try { var_dump($this->v); } catch (Error $e) { echo $e->getMessage(), "\n"; } try { var_dump($this->x); } catch (Error $e) { echo $e->getMessage(), "\n"; } } }
(new B)->u();
class C2 { private $p { get => 1; } public function __get($n) { return "magic $n"; } }
var_dump((new C2)->p);
--EXPECTF--
int(2)
int(5)
int(11)
Cannot access private property A::$x
Cannot access protected property A::$v
Cannot access private property A::$x
bool(false)
bool(false)
Cannot access protected property A::$v
Cannot access private property A::$x
Cannot access private property A::$x
Cannot access private property A::$x
Cannot access private property A::$x
Cannot access private property A::$x
int(5)

Warning: Undefined property: B::$x in %s on line 17
NULL
string(7) "magic p"
