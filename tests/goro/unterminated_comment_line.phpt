--TEST--
Unterminated comment: on its starting line
--FILE--
<?php
echo 1;
/* Foo
Bar

baz
--EXPECTF--

Parse error: Unterminated comment starting line 3 in %s on line 3
