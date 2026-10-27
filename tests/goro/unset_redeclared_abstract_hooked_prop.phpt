--TEST--
A plain redeclaration of an abstract hooked property is no hooked property: unset() works
--FILE--
<?php
interface I { public string $foo { get; } }
abstract class A implements I { abstract public string $bar { get; } }
class B extends A { public string $bar; public string $foo; }
$rp = new ReflectionProperty(B::class, 'bar');
var_dump($rp->hasHooks(), $rp->isVirtual(), $rp->isAbstract());
$rp = new ReflectionProperty(B::class, 'foo');
var_dump($rp->hasHooks(), $rp->isVirtual(), $rp->isAbstract());
$b = new B;
try { unset($b->foo); echo "ok foo\n"; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { unset($b->bar); echo "ok bar\n"; } catch (Error $e) { echo $e->getMessage(), "\n"; }
abstract class P { abstract public int $x { get; set { $this->x = $value * 2; } } }
class Q extends P { public int $x { get => $this->x; } }
$q = new Q; $q->x = 2; var_dump($q->x);
try { unset($q->x); echo "ok x\n"; } catch (Error $e) { echo $e->getMessage(), "\n"; }

?>
--EXPECTF--
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
ok foo
ok bar
int(4)
Cannot unset hooked property Q::$x
