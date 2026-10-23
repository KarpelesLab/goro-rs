--TEST--
php://temp takes a new resource when it moves to a temporary file
--FILE--
<?php
// php://temp moves to a temporary file stream (a new resource) once it
// reaches its memory limit.
$m = fopen('php://temp/maxmemory:16', 'w+');
var_dump($m);
fwrite($m, str_repeat('x', 10));
var_dump(fopen('php://memory', 'r'));
fwrite($m, str_repeat('y', 10));
var_dump(fopen('php://memory', 'r'));
fwrite($m, str_repeat('z', 10));
var_dump(fopen('php://memory', 'r'));
rewind($m);
var_dump(stream_get_contents($m));
$t = fopen('php://temp', 'w+');
fwrite($t, str_repeat('x', 2 * 1024 * 1024));
var_dump(fopen('php://memory', 'r'));
--EXPECT--
resource(5) of type (stream)
resource(7) of type (stream)
resource(9) of type (stream)
resource(10) of type (stream)
string(30) "xxxxxxxxxxyyyyyyyyyyzzzzzzzzzz"
resource(14) of type (stream)
