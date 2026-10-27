--TEST--
fopen() with an invalid mode: the warning shows the mode's bytes (up to a NUL)
--FILE--
<?php
var_dump(fopen(__FILE__, "\xff\xfe"));
var_dump(fopen(__FILE__, "z\0r"));
var_dump(fopen(__FILE__, ""));
var_dump(fopen("php://memory", "\xffr"));
--EXPECTF--

Warning: fopen(%s): Failed to open stream: `ÿþ' is not a valid mode for fopen in %s on line 2
bool(false)

Warning: fopen(%s): Failed to open stream: `z' is not a valid mode for fopen in %s on line 3
bool(false)

Warning: fopen(%s): Failed to open stream: `' is not a valid mode for fopen in %s on line 4
bool(false)
resource(5) of type (stream)
