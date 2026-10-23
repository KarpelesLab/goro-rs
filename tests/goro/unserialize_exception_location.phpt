--TEST--
An unserialized exception is created where unserialize() runs (file, line, trace), then gets the payload's props
--FILE--
<?php
function f() { return unserialize('O:9:"Exception":0:{}'); }
$u3 = f();
var_dump($u3->getFile() === __FILE__, $u3->getLine());
echo $u3->getTraceAsString(), "\n";
$u4 = unserialize('O:9:"Exception":1:{s:7:"' . "\0*\0" . 'line";i:9;}');
var_dump($u4->getLine());
echo $u4->getTraceAsString(), "\n";
--EXPECTF--
bool(true)
int(2)
#0 %s(2): unserialize('O:9:"Exception"...')
#1 %s(3): f()
#2 {main}
int(9)
#0 %s(6): unserialize('O:9:"Exception"...')
#1 {main}
