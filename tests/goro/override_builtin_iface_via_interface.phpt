--TEST--
#[Override] on methods of builtin interfaces extended by user interfaces
--FILE--
<?php
interface R extends Countable, IteratorAggregate {}
interface Col extends R, ArrayAccess {}
class AC implements Col, Stringable {
    #[Override] public function offsetExists(mixed $o): bool { return true; }
    #[Override] public function offsetGet(mixed $o): mixed { return 1; }
    #[Override] public function offsetSet(mixed $o, mixed $v): void {}
    #[Override] public function offsetUnset(mixed $o): void {}
    #[Override] public function count(): int { return 0; }
    #[Override] public function getIterator(): Traversable { return new ArrayIterator([]); }
    #[Override] public function __toString(): string { return ''; }
}
interface R2 extends Countable {}
class A implements R2 { #[Override] public function count(): int { return 0; } }
class B implements Countable { #[Override] public function count(): int { return 0; } }
echo "ok\n";

?>
--EXPECT--
ok
