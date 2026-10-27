--TEST--
#[\Override] on an interface method redeclaring a method of the one internal interface it extends
--FILE--
<?php
interface I extends \IteratorAggregate { #[\Override] public function getIterator(): \Traversable; }
interface J extends \Countable { #[\Override] public function count(): int; }
interface K extends \ArrayAccess { #[\Override] public function offsetGet(mixed $o): mixed; }
interface L extends \Iterator { #[\Override] public function current(): mixed; }
interface U { public function u(): void; }
interface M extends U { #[\Override] public function u(): void; }
echo "ok\n";
interface N extends \Countable { #[\Override] public function nope(): int; }
?>
--EXPECTF--
ok

Fatal error: N::nope() has #[\Override] attribute, but no matching parent method exists in %s on line 9
