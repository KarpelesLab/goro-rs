--TEST--
A backed hooked property implicitly implements the abstract hooks it doesn't define
--FILE--
<?php
interface I1 { public $x { get; } }
class A1 implements I1 { public $x { set => $value; } }
interface I2 { public $x { get; set; } }
class A2 implements I2 { public $x { get => $this->x; } }
class A3 implements I2 { public $x { set => $value; } }
abstract class B { abstract public $x { get; set; } }
class A4 extends B { public $x { set => $value * 2; } }
class A5 implements I2 { public $x { &get => $this->x; } }
interface I3 { public $x { set; } }
class A6 implements I3 { public $x { get => $this->x; } }
foreach (['A1', 'A2', 'A3', 'A4', 'A5', 'A6'] as $c) {
    $a = new $c;
    $a->x = 3;
    var_dump($a->x);
}
--EXPECT--
int(3)
int(3)
int(3)
int(6)
int(3)
int(3)
