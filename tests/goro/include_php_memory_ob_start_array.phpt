--TEST--
include of php://memory / php://temp (an empty file); ob_start([1, 2]) reports the first array member
--FILE--
<?php
var_dump(include "php://memory");
var_dump(include "php://temp");
var_dump(@ob_start([1, 2]));
var_dump(ob_start([1, 2]));
--EXPECTF--
int(1)
int(1)
bool(false)

Warning: ob_start(): first array member is not a valid class name or object in %s on line 5

Notice: ob_start(): Failed to create buffer in %s on line 5
bool(false)
