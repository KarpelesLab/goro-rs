--TEST--
The CLI registers $argv / $argc (and $_SERVER's) even with register_argc_argv=0
--FILE--
<?php
$code = 'var_dump($argc, $argv, $_SERVER["argc"], $_SERVER["argv"] === $argv, ini_get("register_argc_argv"));';
echo shell_exec(escapeshellarg(PHP_BINARY) . ' -n -d register_argc_argv=0 -r ' . escapeshellarg($code) . ' a b');
?>
--EXPECT--
int(3)
array(3) {
  [0]=>
  string(19) "Standard input code"
  [1]=>
  string(1) "a"
  [2]=>
  string(1) "b"
}
int(3)
bool(true)
string(1) "0"
