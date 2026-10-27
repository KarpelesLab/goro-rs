--TEST--
proc_open() file descriptor with an invalid mode
--FILE--
<?php
$spec = [0 => ['pipe', 'r'], 1 => ['file', '/nonexistent-dir-x/out', 'z']];
var_dump(proc_open('true', $spec, $pipes));
--EXPECTF--

Warning: proc_open(/nonexistent-dir-x/out): Failed to open stream: `z' is not a valid mode for fopen in %s on line 3
bool(false)
