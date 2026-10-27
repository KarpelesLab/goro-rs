--TEST--
Spawned commands inherit the CLI's ignored SIGPIPE, not the signal mask nor goro's RLIMIT_AS cap
--EXTENSIONS--
pcntl
posix
--SKIPIF--
<?php if (!is_readable('/proc/self/status')) die('skip needs /proc'); ?>
--FILE--
<?php
function sig($status) {
    preg_match('/SigIgn:\s*([0-9a-f]+)/', $status, $i);
    preg_match('/SigBlk:\s*([0-9a-f]+)/', $status, $b);
    $ign = hexdec(substr($i[1], -8));
    $blk = hexdec(substr($b[1], -8));
    return sprintf("pipe-ignored=%d usr1-ignored=%d hup-blocked=%d", ($ign >> (SIGPIPE - 1)) & 1, ($ign >> (SIGUSR1 - 1)) & 1, ($blk >> (SIGHUP - 1)) & 1);
}
echo "shell_exec: ", sig(shell_exec('cat /proc/self/status')), "\n";
exec('cat /proc/self/status', $o); echo "exec: ", sig(implode("\n", $o)), "\n";
$p = proc_open(['cat', '/proc/self/status'], [1 => ['pipe', 'w']], $pipes);
echo "proc_open: ", sig(stream_get_contents($pipes[1])), "\n"; proc_close($p);
$p = popen('cat /proc/self/status', 'r'); echo "popen: ", sig(stream_get_contents($p)), "\n"; pclose($p);

pcntl_signal(SIGUSR1, SIG_IGN);
pcntl_sigprocmask(SIG_BLOCK, [SIGHUP]);
echo "after: ", sig(shell_exec('cat /proc/self/status')), "\n";
// (the argv form: /bin/sh as dash would reset the mask of its child)
$p = proc_open(['cat', '/proc/self/status'], [1 => ['pipe', 'w']], $pipes);
echo "after proc_open: ", sig(stream_get_contents($pipes[1])), "\n"; proc_close($p);
pcntl_sigprocmask(SIG_UNBLOCK, [SIGHUP]);
pcntl_signal(SIGPIPE, SIG_DFL);
echo "dfl: ", sig(shell_exec('cat /proc/self/status')), "\n";

// A writer into a closed pipe gets EPIPE rather than being killed.
pcntl_signal(SIGPIPE, SIG_IGN);
$p = proc_open(['sh', '-c', 'sleep 0.2; echo x 2>/dev/null; exit 7'], [1 => ['pipe', 'w']], $pipes);
fclose($pipes[1]);
$st = proc_close($p);
var_dump($st);

// goro's own 2 GiB address-space cap is not passed on.
var_dump(trim(shell_exec('ulimit -v')) !== '2097152');
// nor does posix_getrlimit() show it
$l = posix_getrlimit(POSIX_RLIMIT_AS)[0];
$u = trim(shell_exec('ulimit -v'));
var_dump($u === 'unlimited' ? $l === 'unlimited' : $l == $u * 1024);
?>
--EXPECT--
shell_exec: pipe-ignored=1 usr1-ignored=0 hup-blocked=0
exec: pipe-ignored=1 usr1-ignored=0 hup-blocked=0
proc_open: pipe-ignored=1 usr1-ignored=0 hup-blocked=0
popen: pipe-ignored=1 usr1-ignored=0 hup-blocked=0
after: pipe-ignored=1 usr1-ignored=1 hup-blocked=0
after proc_open: pipe-ignored=1 usr1-ignored=1 hup-blocked=1
dfl: pipe-ignored=0 usr1-ignored=1 hup-blocked=0
int(7)
bool(true)
bool(true)
