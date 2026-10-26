--TEST--
Two traits defining the same hooked property conflict as properties
--FILE--
<?php
trait T1 {
    public $x { get => 'T1'; }
}
trait T2 {
    public $x { get => 'T2'; }
}
class A {
    use T1, T2;
}
--EXPECTF--

Fatal error: T1 and T2 define the same hooked property ($x) in the composition of A. Conflict resolution between hooked properties is currently not supported. Class was composed in %s on line 8
