--TEST--
assert(...$args) with a single unpacked argument: the added description is a positional argument after unpacking
--FILE--
<?php
try { assert(...[true]); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECTF--

Fatal error: Cannot use positional argument after argument unpacking in %s on line 2
