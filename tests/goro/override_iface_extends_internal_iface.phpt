--TEST--
#[\Override] on an interface method of an internal interface it extends (interface I extends IteratorAggregate)
--FILE--
<?php
interface CI extends \IteratorAggregate {
    #[\Override]
    public function getIterator(): \Traversable;
}
interface CJ extends \Countable, \ArrayAccess {
    #[\Override] public function count(): int;
    #[\Override] public function offsetGet(mixed $o): mixed;
}
interface CK extends \Stringable { #[\Override] public function __toString(): string; }
interface CL extends \JsonSerializable { #[\Override] public function jsonSerialize(): mixed; }
interface CM extends \Iterator { #[\Override] public function current(): mixed; }
class Q implements CI { #[\Override] public function getIterator(): \Traversable { return new ArrayIterator([1]); } }
foreach (new Q as $v) var_dump($v);
echo "ok\n";

?>
--EXPECTF--
int(1)
ok
