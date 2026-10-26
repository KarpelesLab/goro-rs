--TEST--
A Generator object compared loosely with a number is 1, with a notice
--FILE--
<?php
function g() { yield 1; }
$gen = g();
var_dump(in_array($gen, [1]));
var_dump(array_search($gen, [5, 1]));
var_dump($gen == 1, 1.0 == $gen, $gen == 2);
--EXPECTF--

Notice: Object of class Generator could not be converted to int in %s on line 4
bool(true)

Notice: Object of class Generator could not be converted to int in %s on line 5

Notice: Object of class Generator could not be converted to int in %s on line 5
int(1)

Notice: Object of class Generator could not be converted to int in %s on line 6

Notice: Object of class Generator could not be converted to float in %s on line 6

Notice: Object of class Generator could not be converted to int in %s on line 6
bool(true)
bool(true)
bool(false)
