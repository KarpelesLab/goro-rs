--TEST--
instanceof and typed parameters at sites meeting many classes (remembered answers)
--FILE--
<?php
interface Shape {}
interface Round extends Shape {}
abstract class Base implements Shape {}
class A extends Base {}
class B extends A implements Round {}
class C extends Base {}
class D {}
class E extends D implements Round {}
class_alias('A', 'AliasA');
function kinds(object $o): string {
    $r = '';
    $r .= $o instanceof Shape ? 'S' : '-';
    $r .= $o instanceof Round ? 'R' : '-';
    $r .= $o instanceof Base ? 'B' : '-';
    $r .= $o instanceof A ? 'A' : '-';
    $r .= $o instanceof AliasA ? 'a' : '-';
    $r .= $o instanceof D ? 'D' : '-';
    $r .= $o instanceof Missing ? 'M' : '-';
    return $r;
}
function takesShape(Shape $s): string { return get_class($s); }
function takesBase(Base $b): string { return get_class($b); }
$objs = [new A, new B, new C, new D, new E, new stdClass, new ArrayObject([])];
for ($round = 0; $round < 3; $round++) {
    foreach ($objs as $o) {
        echo get_class($o), ' ', kinds($o), "\n";
    }
}
foreach ([new A, new B, new C, new E] as $o) {
    echo takesShape($o), "\n";
}
foreach ([new A, new B, new C, new E] as $o) {
    try {
        echo takesBase($o), "\n";
    } catch (TypeError $e) {
        echo "TypeError\n";
    }
}
// a class declared after the sites ran
eval('class F extends B {} class Missing {}');
foreach ([new F, new A] as $o) {
    echo get_class($o), ' ', kinds($o), "\n";
}
?>
--EXPECT--
A S-BAa--
B SRBAa--
C S-B----
D -----D-
E SR---D-
stdClass -------
ArrayObject -------
A S-BAa--
B SRBAa--
C S-B----
D -----D-
E SR---D-
stdClass -------
ArrayObject -------
A S-BAa--
B SRBAa--
C S-B----
D -----D-
E SR---D-
stdClass -------
ArrayObject -------
A
B
C
E
A
B
C
TypeError
F SRBAa--
A S-BAa--
