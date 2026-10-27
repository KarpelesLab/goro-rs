--TEST--
Commands run by exec() & co and proc_open(): output, status, inherited state
--SKIPIF--
<?php if (PHP_OS_FAMILY !== "Linux" || !function_exists("pcntl_sigprocmask")) die("skip Linux with pcntl only"); ?>
--FILE--
<?php
// Commands run by exec() & co and proc_open(): output, status, the state
// they inherit (an empty signal mask for the exec() family), environment,
// working directory, failures
$blk = 'grep -E "^SigBlk" /proc/self/status';
pcntl_sigprocmask(SIG_BLOCK, [SIGUSR1]);
exec($blk, $out, $rc);
var_dump($out, $rc);
echo shell_exec($blk);
system($blk . '; exit 3', $rc);
var_dump($rc);
pcntl_sigprocmask(SIG_UNBLOCK, [SIGUSR1]);
passthru('echo passthru; exit 4', $rc);
var_dump($rc);
var_dump(exec('printf "a\nb  \n\n"', $lines), $lines);
var_dump(shell_exec('true'), shell_exec('echo -n x'));
var_dump(exec('yes | head -1'));
$p = popen('cat; echo done', 'w');
fwrite($p, "to cat\n");
var_dump(pclose($p));
$p = popen('echo from popen; exit 5', 'r');
var_dump(fgets($p), pclose($p));
$h = proc_open(['sh', '-c', 'echo "$FOO" "$PWD"; read x; echo "got $x" >&2; exit 7'], [0 => ['pipe', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes, '/tmp', ['FOO' => 'bar', 'PATH' => getenv('PATH')]);
fwrite($pipes[0], "input\n");
fclose($pipes[0]);
echo stream_get_contents($pipes[1]), stream_get_contents($pipes[2]);
fclose($pipes[1]);
fclose($pipes[2]);
var_dump(proc_close($h));
var_dump(getenv('FOO'), getcwd() !== '/tmp');
$h = @proc_open(['/nonexistent/prog'], [], $pipes);
var_dump($h);
var_dump(exec('/nonexistent/prog 2>/dev/null', $o3, $rc3), $rc3);
// the parent's handlers do not run for (nor reach) the children
pcntl_signal(SIGUSR2, function () { echo "parent handler\n"; });
var_dump(exec('kill -USR2 $$ 2>/dev/null; echo survived'));
$h = proc_open('kill -USR2 $$; echo survived too', [1 => ['pipe', 'w']], $pipes);
echo stream_get_contents($pipes[1]);
fclose($pipes[1]);
var_dump(proc_close($h));
pcntl_signal_dispatch();
echo "end\n";
--EXPECT--
array(1) {
  [0]=>
  string(24) "SigBlk:	0000000000000000"
}
int(0)
SigBlk:	0000000000000000
SigBlk:	0000000000000000
int(3)
passthru
int(4)
string(0) ""
array(3) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
  [2]=>
  string(0) ""
}
NULL
string(1) "x"
yes: standard output: Broken pipe
string(1) "y"
to cat
done
int(0)
string(11) "from popen
"
int(5)
bar /tmp
got input
int(7)
bool(false)
bool(true)
bool(false)
string(0) ""
int(127)
string(0) ""
int(12)
end
