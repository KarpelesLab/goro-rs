--TEST--
Unparenthesized arrow function in a pipe: on the pipe's line
--FILE--
<?php
$x = 1;
$r = $x

    |> fn($x) => $x + 1;
--EXPECTF--

Fatal error: Arrow functions on the right hand side of |> must be parenthesized in %s on line 3
