--TEST--
Tentative return type deprecations follow the interface method order
--FILE--
<?php
class A implements ArrayAccess { function offsetGet($o) {} function offsetSet($o, $v) {} function offsetExists($o) {} function offsetUnset($o) {} } echo "ok";
echo "end\n";
--EXPECTF--

Deprecated: Return type of A::offsetExists($o) should either be compatible with ArrayAccess::offsetExists(mixed $offset): bool, or the #[\ReturnTypeWillChange] attribute should be used to temporarily suppress the notice in %s on line 2

Deprecated: Return type of A::offsetGet($o) should either be compatible with ArrayAccess::offsetGet(mixed $offset): mixed, or the #[\ReturnTypeWillChange] attribute should be used to temporarily suppress the notice in %s on line 2

Deprecated: Return type of A::offsetSet($o, $v) should either be compatible with ArrayAccess::offsetSet(mixed $offset, mixed $value): void, or the #[\ReturnTypeWillChange] attribute should be used to temporarily suppress the notice in %s on line 2

Deprecated: Return type of A::offsetUnset($o) should either be compatible with ArrayAccess::offsetUnset(mixed $offset): void, or the #[\ReturnTypeWillChange] attribute should be used to temporarily suppress the notice in %s on line 2
okend
