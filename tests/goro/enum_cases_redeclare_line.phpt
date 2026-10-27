--TEST--
Redeclaring an enum's tryFrom(): reported on the enum's line
--FILE--
<?php

enum Foo: string
{
    case Bar = 'b';

    public static function tryFrom(string $s): ?static {
        return null;
    }
}
--EXPECTF--

Fatal error: Cannot redeclare Foo::tryfrom() in %s on line 3
