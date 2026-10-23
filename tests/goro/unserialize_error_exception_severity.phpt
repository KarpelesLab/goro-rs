--TEST--
unserialize(): an ErrorException starts with severity E_ERROR
--FILE--
<?php
$e = unserialize('O:14:"ErrorException":0:{}');
var_dump($e->getSeverity());
echo str_replace("\0", '~', implode(',', array_keys((array)$e))), "\n";
class MyEE extends ErrorException {}
var_dump(unserialize('O:4:"MyEE":0:{}')->getSeverity());
--EXPECT--
int(1)
~*~message,~Exception~string,~*~code,~*~file,~*~line,~Exception~trace,~Exception~previous,~*~severity
int(1)
