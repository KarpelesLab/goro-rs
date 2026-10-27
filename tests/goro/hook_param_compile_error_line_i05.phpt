--TEST--
Errors about a set hook's parameter: the hook's line, or for incompatible types the line its body ends on (i05)
--FILE--
<?php
class A {
    public int $a {
        get => 1;
        set(
            string $v
        ) {}
    }
}
--EXPECTF--

Fatal error: Type of parameter $v of hook A::$a::set must be compatible with property type in %s on line 7
