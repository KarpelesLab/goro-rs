--TEST--
Hard timeout in an internal function: the fatal error names the executing file and line
--SKIPIF--
<?php
if (getenv("SKIP_SLOW_TESTS")) die("skip slow test");
if (substr(PHP_OS, 0, 3) == 'WIN') die("skip not for Windows");
?>
--INI--
max_execution_time=1
hard_timeout=1
pcre.backtrack_limit=1000000000
pcre.recursion_limit=1000000000
pcre.jit=0
--FILE--
<?php
$s = str_repeat("a", 64) . "!";
preg_match("/^(\\w+\\s?)*$/", $s);
?>
--EXPECTF--
Fatal error: Maximum execution time of 1+1 seconds exceeded (terminated) in %shard_timeout_location.php on line 3
