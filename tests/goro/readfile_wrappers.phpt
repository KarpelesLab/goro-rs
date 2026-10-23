--TEST--
readfile() opens stream wrapper URLs (data:, php://filter)
--FILE--
<?php
// readfile() opens stream wrapper URLs like fopen()
var_dump(readfile('data:,readfile-test')); echo "\n";
var_dump(readfile('data://text/plain;base64,SGVsbG8=')); echo "\n";
$f = tempnam(sys_get_temp_dir(), 'rf'); file_put_contents($f, "plain file\n");
var_dump(readfile($f));
var_dump(readfile('php://filter/read=string.toupper/resource=' . $f));
unlink($f);
--EXPECT--
readfile-testint(13)

Helloint(5)

plain file
int(11)
PLAIN FILE
int(11)
