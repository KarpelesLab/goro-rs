--TEST--
Recursive mkdir() of an empty path fails with Invalid path
--FILE--
<?php
var_dump(@mkdir('', 0777, true));
var_dump(error_get_last()['message']);
var_dump(@mkdir(''));
var_dump(error_get_last()['message']);
var_dump(is_dir(''));

?>
--EXPECT--
bool(false)
string(21) "mkdir(): Invalid path"
bool(false)
string(34) "mkdir(): No such file or directory"
bool(false)
