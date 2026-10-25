--TEST--
ReflectionProperty::setRawValue() with a set hook inherited by a redeclaration
--FILE--
<?php
function t($f) { try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }

class A { public int $x = 0 { set { echo "set\n"; $this->x = $value; } get { echo "get\n"; return $this->x; } } }
$a = new A; $r = new ReflectionProperty('A', 'x');
$r->setValue($a, 1); var_dump($r->getValue($a)); $r->setRawValue($a, 2); var_dump($r->getRawValue($a));
class B extends A { public int $x { get { echo "B get\n"; return parent::$x::get(); } } }
$b = new B; $r2 = new ReflectionProperty('B', 'x'); $r2->setRawValue($b, 8); var_dump($r2->getRawValue($b), $b->x);
var_dump($r->getRawValue($b));

--EXPECT--
set
get
int(1)
int(2)
B get
get
int(8)
int(8)
int(8)
