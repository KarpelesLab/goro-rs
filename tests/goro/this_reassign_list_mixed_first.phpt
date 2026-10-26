--TEST--
A keyed/unkeyed list mix is reported before a nested $this target
--FILE--
<?php
class A { function f() { [[$a], "k" => [$this]] = []; } }
--EXPECTF--

Fatal error: Cannot mix keyed and unkeyed array entries in assignments in %s on line 2
