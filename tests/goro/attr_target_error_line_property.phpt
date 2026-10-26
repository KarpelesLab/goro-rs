--TEST--
Attribute target error: reported at the property's line
--FILE--
<?php

class Z {
    #[\Deprecated]
    public $p;
}
--EXPECTF--

Fatal error: Attribute "Deprecated" cannot target property (allowed targets: class, function, method, class constant, constant) in %s on line 5
