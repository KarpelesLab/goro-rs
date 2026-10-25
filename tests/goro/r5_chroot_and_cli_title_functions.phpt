--TEST--
chroot() exists; CLI process title functions belong to standard
--FILE--
<?php
var_dump(function_exists('chroot'));
foreach (['chroot', 'cli_set_process_title', 'cli_get_process_title'] as $f) {
    var_dump((new ReflectionFunction($f))->getExtensionName());
}
var_dump(in_array('cli_get_process_title', get_extension_funcs('standard')));
try { chroot("a\0b"); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
var_dump(@chroot('/nonexistent-dir-xyz'));
--EXPECT--
bool(true)
string(8) "standard"
string(8) "standard"
string(8) "standard"
bool(true)
chroot(): Argument #1 ($directory) must not contain any null bytes
bool(false)
