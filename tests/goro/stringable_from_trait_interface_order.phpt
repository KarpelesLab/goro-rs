--TEST--
Stringable from a trait's __toString() is added after a class's other interfaces (class_implements, getInterfaceNames)
--FILE--
<?php
interface Arr {}
interface Enum extends Arr, Countable, IteratorAggregate, JsonSerializable { public function __toString(); }
trait T { public function __toString(): string { return 't'; } }
abstract class C implements ArrayAccess, Enum { use T; }
abstract class D implements ArrayAccess, Enum { public function __toString(): string { return 'd'; } }
class E { use T; }
class F implements Countable { use T; public function count(): int { return 0; } }
trait T2 { use T; }
class G implements JsonSerializable { use T2; public function jsonSerialize(): mixed { return 1; } }
foreach (['Enum', 'C', 'D', 'E', 'F', 'G'] as $c) {
    echo $c, ": ", implode(', ', class_implements($c)), "\n";
    echo "  ", implode(', ', (new ReflectionClass($c))->getInterfaceNames()), "\n";
    echo "  ", implode(', ', array_keys((new ReflectionClass($c))->getInterfaces())), "\n";
}

?>
--EXPECTF--
Enum: Arr, Countable, IteratorAggregate, JsonSerializable, Stringable, Traversable
  Arr, Countable, IteratorAggregate, JsonSerializable, Stringable, Traversable
  Arr, Countable, IteratorAggregate, JsonSerializable, Stringable, Traversable
C: ArrayAccess, Enum, Traversable, Stringable, JsonSerializable, IteratorAggregate, Countable, Arr
  ArrayAccess, Enum, Traversable, Stringable, JsonSerializable, IteratorAggregate, Countable, Arr
  ArrayAccess, Enum, Traversable, Stringable, JsonSerializable, IteratorAggregate, Countable, Arr
D: ArrayAccess, Enum, Stringable, Traversable, JsonSerializable, IteratorAggregate, Countable, Arr
  ArrayAccess, Enum, Stringable, Traversable, JsonSerializable, IteratorAggregate, Countable, Arr
  ArrayAccess, Enum, Stringable, Traversable, JsonSerializable, IteratorAggregate, Countable, Arr
E: Stringable
  Stringable
  Stringable
F: Countable, Stringable
  Countable, Stringable
  Countable, Stringable
G: JsonSerializable, Stringable
  JsonSerializable, Stringable
  JsonSerializable, Stringable
