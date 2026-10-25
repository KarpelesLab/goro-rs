--TEST--
Classes implementing internal interfaces (directly, through traits or internal parents)
--FILE--
<?php
class Ok1 implements Countable { function count(): int { return 1; } }
class Ok2 extends ArrayIterator implements Countable {}
abstract class Ab implements IteratorAggregate {}
class Ok3 extends Ab { function getIterator(): Iterator { return new ArrayIterator([]); } }
trait TC { function count(): int { return 2; } }
class Ok4 implements Countable { use TC; }
interface MyIt extends Iterator {}
class Ok5 implements MyIt { function current(): mixed { return 1; } function key(): mixed { return 1; } function next(): void {} function rewind(): void {} function valid(): bool { return false; } }
class Ok6 implements JsonSerializable, ArrayAccess { function jsonSerialize(): mixed { return 1; } function offsetExists($o): bool { return false; } function offsetGet($o): mixed { return 1; } function offsetSet($o, $v): void {} function offsetUnset($o): void {} }
enum E implements JsonSerializable { case A; function jsonSerialize(): mixed { return 1; } }
class Ok7 extends Exception {}
class Ok8 implements Stringable { function __toString(): string { return ''; } }
echo "ok so far\n";
--EXPECT--
ok so far
