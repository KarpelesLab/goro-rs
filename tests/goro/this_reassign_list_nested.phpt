--TEST--
Destructuring into $this is a compile error
--FILE--
<?php
[$a, [$this]] = [1, [2]];
--EXPECTF--

Fatal error: Cannot re-assign $this in %s on line 2
