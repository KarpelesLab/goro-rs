--TEST--
Reflection of enum/interface modifiers, enum method prototypes, internal enums
--FILE--
<?php
enum E: string { case A = 'a'; }
enum U { case X; }
var_dump((new ReflectionClass('E'))->getModifiers(), (new ReflectionClass('U'))->getModifiers());
var_dump((new ReflectionClass('E'))->isFinal());
var_dump((new ReflectionMethod('E', 'from'))->getPrototype()->class);
var_dump((new ReflectionMethod('E', 'tryFrom'))->getPrototype()->class);
var_dump((new ReflectionMethod('E', 'cases'))->getPrototype()->class);
var_dump((new ReflectionMethod('U', 'cases'))->getPrototype()->class);
foreach (['Countable', 'Iterator', 'UnitEnum', 'BackedEnum', 'Traversable', 'Stringable', 'Throwable'] as $i) {
  $r = new ReflectionClass($i);
  echo $i, " ", var_export($r->isAbstract(), true), " ", $r->getModifiers(), "\n";
}
interface MyI { function f(); }
var_dump((new ReflectionClass('MyI'))->isAbstract(), (new ReflectionClass('MyI'))->getModifiers());
foreach (['RoundingMode', 'Random\IntervalBoundary', 'Dom\AdjacentPosition', 'PropertyHookType', 'Uri\UriComparisonMode'] as $c) {
  $r = new ReflectionClass($c);
  echo $c, ": ", implode(",", $r->getInterfaceNames()), " | ";
  echo implode(",", array_map(fn($p) => $p->name, $r->getProperties())), " | ";
  echo $r->getModifiers(), " ", var_export($r->isFinal(), true), "\n";
  $m = $r->getMethod('cases');
  echo "  cases proto: ", $m->getPrototype()->class, "\n";
}
interface I0 {}
interface I2 extends Countable {}
abstract class AC implements Countable {}
class CC implements I0 {}
abstract class AB { abstract function f(); }
class Impl extends AB { function f() {} }
foreach (['I0', 'I2', 'AC', 'CC', 'AB', 'Impl', 'ArrayAccess', 'OuterIterator', 'DateTimeInterface', 'JsonSerializable', 'SplObserver'] as $c) {
  $r = new ReflectionClass($c);
  echo $c, " ", var_export($r->isAbstract(), true), " ", $r->getModifiers(), "\n";
}
--EXPECT--
int(32)
int(32)
bool(true)
string(10) "BackedEnum"
string(10) "BackedEnum"
string(8) "UnitEnum"
string(8) "UnitEnum"
Countable true 0
Iterator true 0
UnitEnum true 0
BackedEnum true 0
Traversable false 0
Stringable true 0
Throwable true 0
bool(true)
int(0)
RoundingMode: UnitEnum | name | 0 false
  cases proto: UnitEnum
Random\IntervalBoundary: UnitEnum | name | 0 false
  cases proto: UnitEnum
Dom\AdjacentPosition: BackedEnum,UnitEnum | name,value | 0 false
  cases proto: UnitEnum
PropertyHookType: BackedEnum,UnitEnum | name,value | 0 false
  cases proto: UnitEnum
Uri\UriComparisonMode: UnitEnum | name | 0 false
  cases proto: UnitEnum
I0 false 0
I2 true 0
AC true 64
CC false 0
AB true 64
Impl false 0
ArrayAccess true 0
OuterIterator true 0
DateTimeInterface true 0
JsonSerializable true 0
SplObserver true 0
