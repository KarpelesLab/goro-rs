--TEST--
proc_open() children inherit the signal mask (pcntl_sigprocmask SIG_BLOCK)
--SKIPIF--
<?php if (!function_exists("pcntl_sigprocmask") || !is_readable("/proc/self/status")) die("skip pcntl + procfs"); ?>
--FILE--
<?php
$old = [];
var_dump(pcntl_sigprocmask(SIG_BLOCK, [SIGTERM], $old));
$p = proc_open(['grep', 'SigBlk', '/proc/self/status'], [1 => ['pipe', 'w']], $pipes);
pcntl_sigprocmask(SIG_SETMASK, $old);
echo stream_get_contents($pipes[1]);
proc_close($p);
$p = proc_open('grep SigBlk /proc/self/status', [1 => ['pipe', 'w']], $pipes);
echo stream_get_contents($pipes[1]);
proc_close($p);

?>
--EXPECT--
bool(true)
SigBlk:	0000000000004000
SigBlk:	0000000000000000
