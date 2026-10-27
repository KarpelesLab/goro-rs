--TEST--
Abstract property without abstract hook: at the end of the last hook
--FILE--
<?php
abstract class Test {
    public abstract $prop {
        get {}
        set {
        }
    }
}
--EXPECTF--

Fatal error: Abstract property Test::$prop must specify at least one abstract hook in %s on line 6
