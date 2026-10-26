--TEST--
Repeated attribute on a method: reported at the method's line
--FILE--
<?php
class C {

    #[\Deprecated]
    #[\Deprecated]
    function m() {}
}
--EXPECTF--

Fatal error: Attribute "Deprecated" must not be repeated in %s on line 6
