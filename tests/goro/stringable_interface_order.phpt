--TEST--
Interface order: Stringable is added after the declared interfaces of a class declaring __toString()
--FILE--
<?php
interface H {}
abstract class E implements H, JsonSerializable { function __toString(): string { return ''; } }
class U extends E { function jsonSerialize(): mixed { return 1; } }
echo implode(',', (new ReflectionClass('E'))->getInterfaceNames()), "\n";
echo implode(',', (new ReflectionClass('U'))->getInterfaceNames()), "\n";
echo implode(',', array_keys(class_implements('U'))), "\n";
interface I1 {} interface I2 extends I1 {}
class A implements I2, Countable { function count(): int { return 0; } }
class B extends A implements IteratorAggregate { function getIterator(): Iterator { return new ArrayIterator([]); } }
echo implode(',', (new ReflectionClass('A'))->getInterfaceNames()), "\n";
echo implode(',', (new ReflectionClass('B'))->getInterfaceNames()), "\n";
class S { function __toString(): string { return ''; } }
class S2 extends S {}
class S3 extends S implements Countable { function count(): int { return 0; } }
echo implode(',', (new ReflectionClass('S2'))->getInterfaceNames()), "\n";
echo implode(',', (new ReflectionClass('S3'))->getInterfaceNames()), "\n";
class T implements H { function __toString(): string { return ''; } }
echo implode(',', (new ReflectionClass('T'))->getInterfaceNames()), "\n";
enum En: string implements H { case A = 'a'; }
echo implode(',', (new ReflectionClass('En'))->getInterfaceNames()), "\n";
interface H2 {}
trait T2 { function __toString(): string { return ''; } }
class A2 implements H2, Countable { use T2; function count(): int { return 0; } }
class B2 extends A2 {}
interface SI2 extends H2 { function __toString(): string; }
class C2 implements SI2 { function __toString(): string { return ''; } }
class D2 extends C2 implements Countable { function count(): int { return 0; } }
foreach (['A2', 'B2', 'SI2', 'C2', 'D2'] as $c) {
    echo $c, ': ', implode(',', (new ReflectionClass($c))->getInterfaceNames()), ' | ', implode(',', array_keys(class_implements($c))), "\n";
}
?>
--EXPECT--
H,JsonSerializable,Stringable
Stringable,JsonSerializable,H
Stringable,JsonSerializable,H
I2,Countable,I1
I2,Countable,I1,IteratorAggregate,Traversable
Stringable
Stringable,Countable
H,Stringable
H,UnitEnum,BackedEnum
A2: H2,Countable,Stringable | H2,Countable,Stringable
B2: Stringable,Countable,H2 | Stringable,Countable,H2
SI2: H2,Stringable | H2,Stringable
C2: SI2,Stringable,H2 | SI2,Stringable,H2
D2: SI2,Stringable,H2,Countable | SI2,Stringable,H2,Countable
