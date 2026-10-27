--TEST--
Trait use in an interface: reported on the use's line
--FILE--
<?php
trait T {}

interface I
{
    public function f();

    use T;
}
--EXPECTF--

Fatal error: Cannot use traits inside of interfaces. T is used in I in %s on line 8
