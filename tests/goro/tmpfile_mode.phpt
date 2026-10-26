--TEST--
tmpfile() is opened r+b
--FILE--
<?php
$t = tmpfile(); fwrite($t, "abc"); rewind($t); var_dump(fread($t, 10), stream_get_meta_data($t)["mode"]); fseek($t, 1); fwrite($t, "Z"); rewind($t); var_dump(fread($t, 10)); fclose($t);
--EXPECT--
string(3) "abc"
string(3) "r+b"
string(3) "aZc"
