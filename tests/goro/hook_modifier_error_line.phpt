--TEST--
Property hook modifier errors are reported on the hook's line
--FILE--
<?php
abstract class Test {
    public abstract $prop {
        get;

        final set;
    }
}
--EXPECTF--

Fatal error: Property hook cannot be both abstract and final in %s on line 6
