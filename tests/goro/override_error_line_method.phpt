--TEST--
#[\Override] without a parent method: reported at the method's line
--FILE--
<?php
class P {}
class C extends P {

    #[\Override]
    function m() {}
}
--EXPECTF--

Fatal error: C::m() has #[\Override] attribute, but no matching parent method exists in %s on line 6
