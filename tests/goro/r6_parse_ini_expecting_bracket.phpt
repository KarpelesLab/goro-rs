--TEST--
parse_ini_string(): an unclosed [ expects ']'
--FILE--
<?php
foreach (['x[', 'x[a', '[', '[a', 'x[a]b'] as $s) {
    var_dump(parse_ini_string($s));
}
--EXPECTF--

Warning: syntax error, unexpected end of file, expecting ']' in Unknown on line 1
 in %s on line 3
bool(false)

Warning: syntax error, unexpected end of file, expecting ']' in Unknown on line 1
 in %s on line 3
bool(false)

Warning: syntax error, unexpected end of file, expecting ']' in Unknown on line 1
 in %s on line 3
bool(false)

Warning: syntax error, unexpected end of file, expecting ']' in Unknown on line 1
 in %s on line 3
bool(false)

Warning: syntax error, unexpected TC_LABEL, expecting '=' in Unknown on line 1
 in %s on line 3
bool(false)
