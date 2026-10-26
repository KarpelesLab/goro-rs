--TEST--
php://temp opened a+ stops appending once it moves to a file
--FILE--
<?php
$t = fopen("php://temp/maxmemory:2", "a+"); fwrite($t,"abc"); fseek($t,1); fwrite($t,"Z"); rewind($t); var_dump(stream_get_contents($t));
$t = fopen("php://temp", "a+"); fwrite($t,"abc"); fseek($t,1); fwrite($t,"Z"); rewind($t); var_dump(stream_get_contents($t));
$t = fopen("php://memory", "a+"); fwrite($t,"abc"); fseek($t,1); fwrite($t,"Z"); rewind($t); var_dump(stream_get_contents($t));
--EXPECT--
string(3) "aZc"
string(4) "abcZ"
string(4) "abcZ"
