--TEST--
Using an internal class as a trait throws
--FILE--
<?php
try { eval("class C { use stdClass; }"); } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
C cannot use stdClass - it is not a trait
