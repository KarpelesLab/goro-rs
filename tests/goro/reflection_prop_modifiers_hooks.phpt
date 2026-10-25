--TEST--
Reflection: implicit protected(set), interface hooked properties are abstract, hook doc comments
--FILE--
<?php
class A { public readonly int $a; protected readonly int $b; private readonly int $c; private $d; protected $e; public private(set) int $f; protected private(set) int $g; public protected(set) readonly int $h; }
foreach (['a','b','c','d','e','f','g','h'] as $p) {
  $r = new ReflectionProperty('A', $p);
  echo $p, " ", var_export($r->isProtectedSet(), true), " ", var_export($r->isPrivateSet(), true), " ", $r->getModifiers(), "\n";
}
interface I { public $p { get; } public int $q { get; set; } }
abstract class B { abstract public $p { get; } public $z { get => 1; } }
foreach ([['I','p'],['I','q'],['B','p'],['B','z']] as [$c,$p]) { $r = new ReflectionProperty($c,$p); var_dump($r->isAbstract(), $r->getModifiers(), $r->isFinal()); }
class H { public $x { /** hookdoc */ get => 1; /** setdoc */ set {} } public $y { get => 2; } }
$r = new ReflectionProperty('H','x');
var_dump($r->getHook(PropertyHookType::Get)->getDocComment(), $r->getHook(PropertyHookType::Set)->getDocComment());
var_dump((new ReflectionProperty('H','y'))->getHook(PropertyHookType::Get)->getDocComment());
var_dump((new ReflectionProperty('H','y'))->getDefaultValue());
var_dump((new ReflectionProperty('H','y'))->hasDefaultValue());
--EXPECTF--
a true false 2177
b false false 130
c false false 132
d false false 4
e false false 2
f false true 4129
g false true 4130
h true false 2177
bool(true)
int(577)
bool(false)
bool(true)
int(577)
bool(false)
bool(true)
int(577)
bool(false)
bool(false)
int(513)
bool(false)
string(14) "/** hookdoc */"
string(13) "/** setdoc */"
bool(false)

Deprecated: ReflectionProperty::getDefaultValue() for a property without a default value is deprecated, use ReflectionProperty::hasDefaultValue() to check if the default value exists in %s on line 14
NULL
bool(false)
