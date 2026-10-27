--TEST--
A constant array literal with an array key is a compile-time 'Illegal offset type' (l09)
--FILE--
<?php
echo "a";
$a = [
  1,
  [
    2,
    [3] => 4,
  ],
];
--EXPECTF--

Fatal error: Illegal offset type in %s on line 4
