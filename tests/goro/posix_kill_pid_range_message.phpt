--TEST--
posix_kill() pid range error: INT_MIN printed through ZEND_LONG_FMT
--FILE--
<?php
try { posix_kill(PHP_INT_MAX, SIGTERM); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
try { posix_kill(PHP_INT_MIN, SIGTERM); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
try { posix_getpgid(-1); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
posix_kill(): Argument #1 ($process_id) must be between 2147483648 and 2147483647
posix_kill(): Argument #1 ($process_id) must be between 2147483648 and 2147483647
