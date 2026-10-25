--TEST--
ReflectionConstant::getAttributes() filters by name and flags
--FILE--
<?php
#[Attribute] class A {}
#[Attribute] class B extends A {}
#[A] #[B] const C = 1;
$r = new ReflectionConstant('C');
var_dump(count($r->getAttributes('nope')), count($r->getAttributes('A')), count($r->getAttributes('B')));
var_dump(count($r->getAttributes('A', ReflectionAttribute::IS_INSTANCEOF)));
var_dump(count($r->getAttributes('\A')), count($r->getAttributes('a')));
foreach ($r->getAttributes('A', ReflectionAttribute::IS_INSTANCEOF) as $a) echo $a->getName(), "\n";
try { $r->getAttributes('A', 5); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $r->getAttributes('NoSuch', ReflectionAttribute::IS_INSTANCEOF); } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
int(0)
int(1)
int(1)
int(2)
int(0)
int(1)
A
B
ReflectionConstant::getAttributes(): Argument #2 ($flags) must be a valid attribute filter flag
Class "NoSuch" not found
