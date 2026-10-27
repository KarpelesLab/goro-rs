--TEST--
Class constant errors are reported on the constant's line
--FILE--
<?php
class A
{
    public const int OK = 1;

    private final const B = 2;
}
--EXPECTF--

Fatal error: Private constant A::B cannot be final as it is not visible to other classes in %s on line 6
