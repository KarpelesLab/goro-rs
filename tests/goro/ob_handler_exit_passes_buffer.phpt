--TEST--
exit() inside a user output handler: the buffer (and what the handler wrote) is passed along, no deprecation
--FILE--
<?php
ob_start(function($b, $p) { exit("exit in cb\n"); });
echo "main\n";
ob_end_flush();
echo "after\n";
--EXPECT--
main
exit in cb
