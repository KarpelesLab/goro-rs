--TEST--
Flexible heredoc whose body starts with an interpolation: no active file
--FILE--
<?php
$var = 1;
echo 1;
var_dump(<<<TEST
$var
 TEST);
--EXPECT--

Parse error: Invalid body indentation level (expecting an indentation level of at least 1) in [no active file] on line 0
