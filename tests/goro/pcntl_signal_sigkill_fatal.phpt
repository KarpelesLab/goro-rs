--TEST--
pcntl_signal() on an uncatchable signal is a fatal error (zend signals sigaction failure)
--EXTENSIONS--
pcntl
--INI--
fatal_error_backtraces=1
--FILE--
<?php
var_dump(pcntl_signal(SIGUSR1, SIG_IGN));
try {
    pcntl_signal(SIGKILL, SIG_IGN);
} catch (Throwable $e) {
    echo "caught\n";
}
echo "not reached\n";
?>
--EXPECTF--
bool(true)

Fatal error: Error installing signal handler for 9 in %s on line %d
Stack trace:
#0 %s(%d): pcntl_signal(9, 1)
#1 {main}
