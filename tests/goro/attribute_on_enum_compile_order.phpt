--TEST--
#[Attribute] on an enum is a compile error reported in source order
--FILE--
<?php
echo "not reached\n";
#[Attribute] class Ok {}
#[Attribute] enum En {}
#[Attribute] interface Ifc {}
--EXPECTF--

Fatal error: Cannot apply #[\Attribute] to enum En in %s on line 4
