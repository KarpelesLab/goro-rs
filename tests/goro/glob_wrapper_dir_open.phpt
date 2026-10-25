--TEST--
glob:// with dir(), rewinddir(), and the stream open it does not support
--FILE--
<?php
$d = sys_get_temp_dir() . "/goro_glob_" . getmypid();
@mkdir($d);
foreach (['a.txt', 'b.txt', 'c.log'] as $f) touch("$d/$f");
$g = dir("glob://$d/*.txt");
var_dump(get_class($g), $g->read(), $g->read(), $g->read());
$g->rewind();
var_dump($g->read());
$g->close();
$h = opendir("glob://$d/*.log");
var_dump(readdir($h), readdir($h));
rewinddir($h);
var_dump(readdir($h));
var_dump(fopen("glob://$d/a.txt", "r"));
var_dump(file_get_contents("glob://$d/a.txt"));
foreach (['a.txt', 'b.txt', 'c.log'] as $f) unlink("$d/$f");
rmdir($d);
?>
--EXPECTF--
string(9) "Directory"
string(5) "a.txt"
string(5) "b.txt"
bool(false)
string(5) "a.txt"
string(5) "c.log"
bool(false)
string(5) "c.log"

Warning: fopen(glob://%s/a.txt): Failed to open stream: wrapper does not support stream open in %s on line %d
bool(false)

Warning: file_get_contents(glob://%s/a.txt): Failed to open stream: wrapper does not support stream open in %s on line %d
bool(false)
