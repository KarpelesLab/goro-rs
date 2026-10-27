--TEST--
#[\NoDiscard] on a property hook is reported on the hook's line
--FILE--
<?php
class A {
    public string $p {
        #[\NoDiscard]

        get {
            return "";
        }
    }
}
--EXPECTF--

Fatal error: #[\NoDiscard] is not supported for property hooks in %s on line 6
