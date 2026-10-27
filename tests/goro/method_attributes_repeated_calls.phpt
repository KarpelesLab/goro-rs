--TEST--
Methods with attributes called many times, through subclasses too: deprecation, #[\NoDiscard] and reflection see their attributes
--FILE--
<?php
#[Attribute]
class Mark { function __construct(public string $v = '') {} }
class Base {
    #[\Deprecated("use other")]
    function old() { return 1; }
    #[\NoDiscard]
    function keep() { return 2; }
    #[Mark('m')]
    function marked() { return 3; }
}
class Child extends Base {}
class Other { #[Mark('o')] function marked() { return 4; } }
$s = 0;
foreach ([new Base, new Child, new Base, new Child] as $o) {
    $s += $o->old();
    $s += $o->keep();
    if (get_class($o) === "Base") $o->keep();
    $s += $o->marked();
}
foreach ([new Base, new Other, new Child] as $o) { $s += $o->marked(); }
var_dump($s);
foreach (['Base', 'Child', 'Other'] as $c) {
    foreach ((new ReflectionMethod($c, 'marked'))->getAttributes() as $a) {
        echo $c, ": ", $a->getName(), " ", $a->newInstance()->v, "\n";
    }
}
var_dump(count((new ReflectionMethod('Child', 'old'))->getAttributes()));
?>
--EXPECTF--

Deprecated: Method Base::old() is deprecated, use other in %s on line 16

Warning: The return value of method Base::keep() should either be used or intentionally ignored by casting it as (void) in %s on line 18

Deprecated: Method Base::old() is deprecated, use other in %s on line 16

Deprecated: Method Base::old() is deprecated, use other in %s on line 16

Warning: The return value of method Base::keep() should either be used or intentionally ignored by casting it as (void) in %s on line 18

Deprecated: Method Base::old() is deprecated, use other in %s on line 16
int(34)
Base: Mark m
Child: Mark m
Other: Mark o
int(1)
