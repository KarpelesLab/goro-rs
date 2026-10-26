--TEST--
proc_open(): 'file' descriptors naming php://stdin, php://stdout, php://stderr and php://fd/N
--SKIPIF--
<?php if (PHP_OS_FAMILY === 'Windows') die('skip not for Windows'); ?>
--FILE--
<?php
// CaptainHook's composer plugin starts its installer like this.
$spec = [
    0 => ['file', 'php://stdin', 'r'],
    1 => ['file', 'php://stdout', 'w'],
    2 => ['file', 'php://stderr', 'w'],
];
$p = proc_open('echo child-out', $spec, $pipes);
$ok = is_resource($p);
$rc = proc_close($p);
var_dump($ok, $pipes, $rc);

$p = proc_open('echo via-fd', [1 => ['file', 'php://fd/1', 'w']], $pipes);
var_dump(proc_close($p));

$f = tempnam(sys_get_temp_dir(), 'po');
$p = proc_open('echo to-file', [1 => ['file', 'file://' . $f, 'w']], $pipes);
proc_close($p);
echo file_get_contents($f);
unlink($f);
?>
--EXPECT--
child-out
bool(true)
array(0) {
}
int(0)
via-fd
int(0)
to-file
