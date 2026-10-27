--TEST--
eval('') returns false
--FILE--
<?php
var_dump(eval(''));
var_dump(eval(' '));
var_dump(eval(';'));
var_dump(eval('?>'));
var_dump(eval('// c'));
var_dump(eval("\n"));
var_dump(eval('/* */'));
--EXPECT--
bool(false)
NULL
NULL
NULL
NULL
NULL
NULL
