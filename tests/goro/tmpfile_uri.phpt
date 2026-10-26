--TEST--
tmpfile(): the file lives in the temporary directory until the stream is closed; its path is the "uri" (PHPUnit's error log capture)
--FILE--
<?php
$f = tmpfile();
$m = stream_get_meta_data($f);
$uri = $m['uri'];
var_dump($m['wrapper_type'], $m['stream_type'], $m['mode']);
var_dump(dirname($uri) === realpath(sys_get_temp_dir()), (bool) preg_match('~/php\w{19}$~', $uri));
var_dump(is_writable($uri));
fwrite($f, "abc");
var_dump(file_get_contents($uri));
fclose($f);
var_dump(file_exists($uri));
?>
--EXPECT--
string(9) "plainfile"
string(5) "STDIO"
string(3) "r+b"
bool(true)
bool(true)
bool(true)
string(3) "abc"
bool(false)
