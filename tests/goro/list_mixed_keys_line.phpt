--TEST--
Mixed keyed/unkeyed list: on the previous element's line
--FILE--
<?php
$array = [];
[
    'a' => $foo,
    'b' => $baz,

    $bar
] = $array;
--EXPECTF--

Fatal error: Cannot mix keyed and unkeyed array entries in assignments in %s on line 5
