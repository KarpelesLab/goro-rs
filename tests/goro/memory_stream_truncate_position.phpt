--TEST--
php://memory / php://temp position after ftruncate() below it
--FILE--
<?php
// php://memory and php://temp: ftruncate() below the position clamps the
// stream's own offset while ftell() keeps stream->position.
foreach (['php://memory', 'php://temp'] as $uri) {
    $m = fopen($uri, 'w+');
    fwrite($m, "0123456789");
    var_dump(ftruncate($m, 3), ftell($m), fgetc($m), feof($m), fwrite($m, "Z"), ftell($m), fstat($m)['size']);
    var_dump(fseek($m, 1, SEEK_CUR), ftell($m));
    rewind($m);
    var_dump(bin2hex(stream_get_contents($m)), ftell($m));
    fwrite($m, "abcdef");
    var_dump(fseek($m, 100), ftell($m), fwrite($m, 'X'), fstat($m)['size'], fseek($m, 2, SEEK_CUR), ftell($m), ftruncate($m, 3), ftell($m), fgetc($m), feof($m));
}
--EXPECT--
bool(true)
int(10)
bool(false)
bool(true)
int(1)
int(11)
int(4)
int(0)
int(12)
string(8) "3031325a"
int(4)
int(0)
int(100)
int(1)
int(101)
int(0)
int(103)
bool(true)
int(103)
bool(false)
bool(true)
bool(true)
int(10)
bool(false)
bool(true)
int(1)
int(11)
int(4)
int(0)
int(12)
string(8) "3031325a"
int(4)
int(0)
int(100)
int(1)
int(101)
int(0)
int(103)
bool(true)
int(103)
bool(false)
bool(true)
