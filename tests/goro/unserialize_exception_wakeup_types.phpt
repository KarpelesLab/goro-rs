--TEST--
Exception::__wakeup() unsets a message that is not a string and a code that is not an int
--FILE--
<?php
function keys($o) { echo str_replace("\0", '~', implode(',', array_keys((array)$o))), "\n"; }
keys(unserialize("O:9:\"Exception\":1:{s:10:\"\0*\0message\";i:5;}"));
keys(unserialize("O:9:\"Exception\":1:{s:7:\"\0*\0code\";s:1:\"5\";}"));
keys(unserialize("O:9:\"Exception\":2:{s:10:\"\0*\0message\";N;s:7:\"\0*\0code\";N;}"));
keys(unserialize("O:5:\"Error\":2:{s:10:\"\0*\0message\";a:0:{}s:7:\"\0*\0code\";d:1.5;}"));
keys(unserialize("O:14:\"ErrorException\":1:{s:7:\"\0*\0code\";b:1;}"));
class MyE extends Exception {}
keys(unserialize("O:3:\"MyE\":1:{s:10:\"\0*\0message\";i:5;}"));
class MyW extends Exception { function __wakeup(): void { echo "wakeup\n"; } }
keys(unserialize("O:3:\"MyW\":1:{s:10:\"\0*\0message\";i:5;}"));
$e = unserialize("O:9:\"Exception\":1:{s:10:\"\0*\0message\";s:2:\"ok\";}");
var_dump($e->getMessage());
--EXPECT--
~Exception~string,~*~code,~*~file,~*~line,~Exception~trace,~Exception~previous
~*~message,~Exception~string,~*~file,~*~line,~Exception~trace,~Exception~previous
~*~message,~Exception~string,~*~code,~*~file,~*~line,~Exception~trace,~Exception~previous
~Error~string,~*~file,~*~line,~Error~trace,~Error~previous
~*~message,~Exception~string,~*~file,~*~line,~Exception~trace,~Exception~previous,~*~severity
~Exception~string,~*~code,~*~file,~*~line,~Exception~trace,~Exception~previous
wakeup
~*~message,~Exception~string,~*~code,~*~file,~*~line,~Exception~trace,~Exception~previous
string(2) "ok"
