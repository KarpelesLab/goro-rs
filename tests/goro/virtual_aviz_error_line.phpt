--TEST--
Virtual property asymmetric visibility error: at the end of the last hook
--FILE--
<?php
class Foo {
    public private(set) int $bar {
        get {
            return 42;
        }
    }
}
--EXPECTF--

Fatal error: Read-only virtual property Foo::$bar must not specify asymmetric visibility in %s on line 6
