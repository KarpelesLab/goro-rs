--TEST--
instanceof a class not declared, run repeatedly: false until a class_alias() names a class of the objects
--FILE--
<?php
class A {}
interface I {}
class B implements I {}
class C implements Countable { function count(): int { return 0; } }
class S { function __toString(): string { return 's'; } }
class D extends A {}

function t($o) {
    return ($o instanceof Later ? 'L' : '-')
        . ($o instanceof \Countable ? 'C' : '-')
        . ($o instanceof Stringable ? 'S' : '-')
        . ($o instanceof NeverDeclared ? 'N' : '-')
        . ($o instanceof I ? 'I' : '-');
}
$objs = [new A, new B, new C, new S, new D, new ArrayObject([])];
for ($i = 0; $i < 3; $i++) {
    echo implode(' ', array_map('t', $objs));
    echo "\n";
}
class_alias('A', 'Later');
for ($i = 0; $i < 3; $i++) {
    echo implode(' ', array_map('t', $objs));
    echo "\n";
}
interface NeverDeclared {}
class E implements NeverDeclared {}
$objs[] = new E;
for ($i = 0; $i < 2; $i++) {
    echo implode(' ', array_map('t', $objs));
    echo "\n";
}
?>
--EXPECT--
----- ----I -C--- --S-- ----- -C---
----- ----I -C--- --S-- ----- -C---
----- ----I -C--- --S-- ----- -C---
L---- ----I -C--- --S-- L---- -C---
L---- ----I -C--- --S-- L---- -C---
L---- ----I -C--- --S-- L---- -C---
L---- ----I -C--- --S-- L---- -C--- ---N-
L---- ----I -C--- --S-- L---- -C--- ---N-
