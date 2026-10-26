--TEST--
Child processes do not inherit goro's own address-space cap (git cannot start its threads under it)
--SKIPIF--
<?php
if (PHP_OS_FAMILY !== 'Linux') die('skip Linux only');
if (trim((string) shell_exec('command -v sh')) === '') die('skip no sh');
?>
--INI--
memory_limit=128M
--FILE--
<?php
// goro caps itself at 2 GiB (RLIMIT_AS); exec(), shell_exec(), popen(),
// proc_open() children get the limit goro was started with back.
$capped = '2097152';
var_dump(trim(shell_exec('ulimit -v')) !== $capped);
exec('ulimit -v', $out);
var_dump($out[0] !== $capped);
$h = popen('ulimit -v', 'r');
var_dump(trim(fgets($h)) !== $capped);
pclose($h);
$p = proc_open('ulimit -v', [1 => ['pipe', 'w']], $pipes);
var_dump(trim(stream_get_contents($pipes[1])) !== $capped);
proc_close($p);
?>
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(true)
