--TEST--
Errors about a set hook's parameter: the hook's line, or for incompatible types the line its body ends on (i01)
--FILE--
<?php
class A {
    public int $a {
        set(
            int $v,
            $w
        ) {}
    }
}
--EXPECTF--

Fatal error: set hook of property A::$a must accept exactly one parameters in %s on line 4
