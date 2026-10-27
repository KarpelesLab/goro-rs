--TEST--
No ${ deprecation when the file fails to parse
--FILE--
<?php
$var = 1;
function f() {
$x = <<<TEST
${var}
 TEST;
}
--EXPECT--

Parse error: Invalid body indentation level (expecting an indentation level of at least 1) in [no active file] on line 0
