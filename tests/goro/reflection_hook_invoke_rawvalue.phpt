--TEST--
Reflection: hook invoke() errors, setRawValue() without a set hook, getDefaultValue() of a virtual property
--FILE--
<?php
class A { public readonly int $x; } var_dump((new ReflectionProperty('A','x'))->isProtectedSet());
interface I { public $p { get; } } $p = new ReflectionProperty('I','p'); var_dump($p->isAbstract(), $p->getModifiers());
class B { public $x { /** hookdoc */ get => 1; } } var_dump((new ReflectionProperty('B','x'))->getHook(PropertyHookType::Get)->getDocComment());
class D { public int $x = 0 { set(int $v) { $this->x = $v; } } }
$s = (new ReflectionProperty('D','x'))->getHook(PropertyHookType::Set);
try { $s->invoke(new D, "nope"); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $s->invoke(new D); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
class E { public $v { get => 3; } public readonly int $ro; function __construct() { $this->ro = 1; } }
try { (new ReflectionProperty('E','v'))->setRawValue(new E, 1); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { (new ReflectionProperty('E','ro'))->setRawValue(new E, 2); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
class F { public $x { get => 1; } }
var_dump((new ReflectionProperty('F','x'))->getDefaultValue());
--EXPECTF--
bool(true)
bool(true)
int(577)
string(14) "/** hookdoc */"
TypeError: D::$x::set(): Argument #1 ($v) must be of type int, string given
ArgumentCountError: Too few arguments to function D::$x::set(), 0 passed and exactly 1 expected
Error: Property E::$v is read-only
Error: Cannot modify readonly property E::$ro

Deprecated: ReflectionProperty::getDefaultValue() for a property without a default value is deprecated, use ReflectionProperty::hasDefaultValue() to check if the default value exists in %s on line 13
NULL
