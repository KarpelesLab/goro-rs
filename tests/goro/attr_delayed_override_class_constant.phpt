--TEST--
A delayed Override on a class constant throws on newInstance()
--FILE--
<?php
class A { #[DelayedTargetValidation] #[Override] const X = 1; } $r = new ReflectionClassConstant("A", "X"); foreach ($r->getAttributes() as $a) { try { $a->newInstance(); echo "ok\n"; } catch (Error $e) { echo $e->getMessage(), "\n"; } }
--EXPECT--
ok
Attribute "Override" cannot target class constant (allowed targets: method, property)
