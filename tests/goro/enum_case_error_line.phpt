--TEST--
Enum case errors are reported on the case's line
--FILE--
<?php
enum Foo: int
{
    case A = 1;

    case Bar;
}
--EXPECTF--

Fatal error: Case Bar of backed enum Foo must have a value in %s on line 6
