--TEST--
instanceof answers stay right as classes, aliases and interfaces are declared later
--FILE--
<?php
interface I {}
interface J extends I {}
class A implements J {}
class B extends A {}
function check($o, $names) {
    $r = [];
    foreach ($names as $n) {
        $r[] = $n . '=' . (int) ($o instanceof $n);
    }
    echo implode(' ', $r), "\n";
}
$names = ['I', 'J', 'A', 'B', 'K', 'Alias', 'Later', 'Stringable', 'Traversable'];
$b = new B;
// many sites and classes: past the per-site caches
foreach ([1, 2] as $_) {
    check($b, $names);
    check(new A, $names);
}
class_alias('J', 'Alias');
check($b, $names);
interface K {}
class Later extends B implements K {}
check($b, $names);
check(new Later, $names);
class_alias('ArrayIterator', 'K2');
$it = new ArrayIterator([]);
foreach ([1, 2] as $_) {
    var_dump($it instanceof K2, $it instanceof Traversable, $it instanceof Countable, $it instanceof I);
}
class S { function __toString(): string { return ''; } }
class T extends S {}
foreach ([1, 2] as $_) {
    var_dump(new T instanceof Stringable, new B instanceof Stringable);
}
enum E: int implements I { case X = 1; }
foreach ([1, 2] as $_) {
    var_dump(E::X instanceof UnitEnum, E::X instanceof BackedEnum, E::X instanceof I, E::X instanceof J);
}
?>
--EXPECT--
I=1 J=1 A=1 B=1 K=0 Alias=0 Later=0 Stringable=0 Traversable=0
I=1 J=1 A=1 B=0 K=0 Alias=0 Later=0 Stringable=0 Traversable=0
I=1 J=1 A=1 B=1 K=0 Alias=0 Later=0 Stringable=0 Traversable=0
I=1 J=1 A=1 B=0 K=0 Alias=0 Later=0 Stringable=0 Traversable=0
I=1 J=1 A=1 B=1 K=0 Alias=1 Later=0 Stringable=0 Traversable=0
I=1 J=1 A=1 B=1 K=0 Alias=1 Later=0 Stringable=0 Traversable=0
I=1 J=1 A=1 B=1 K=1 Alias=1 Later=1 Stringable=0 Traversable=0
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
bool(false)
bool(true)
bool(false)
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
bool(true)
bool(true)
bool(false)
