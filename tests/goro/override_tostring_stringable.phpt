--TEST--
#[\Override] on __toString() of a class without a parent: it implements Stringable
--FILE--
<?php
final class A { #[\Override] public function __toString(): string { return 'A'; } }
var_dump((string)new A, new A instanceof Stringable);
interface I { #[\Override] public function __toString(): string; }
echo "ok\n";
--EXPECT--
string(1) "A"
bool(true)
ok
