--TEST--
Attributes on multiple constants: reported at the declaration's line
--FILE--
<?php

#[A]
const Z = 3, W = 4;
--EXPECTF--

Fatal error: Cannot apply attributes to multiple constants at once in %s on line 4
