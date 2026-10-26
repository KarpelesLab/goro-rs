--TEST--
Repeated attribute on a constant: reported at the declaration's line
--FILE--
<?php

#[\Deprecated]
#[\Deprecated]
const K = 1;
--EXPECTF--

Fatal error: Attribute "Deprecated" must not be repeated in %s on line 5
