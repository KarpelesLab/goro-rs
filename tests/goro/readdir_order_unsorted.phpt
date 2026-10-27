--TEST--
Directory listings keep readdir(3) order
--FILE--
<?php
// readdir(), DirectoryIterator, RecursiveDirectoryIterator and
// scandir(SCANDIR_SORT_NONE) list entries in readdir(3) order, unsorted.
$d = sys_get_temp_dir() . '/goro_readdir_order_' . getmypid();
@mkdir($d);
foreach (['m', 'b', 'zz', 'a', 'k', 'c', 'y', 'q'] as $n) touch("$d/$n");
$h = opendir($d);
$rd = [];
while (($e = readdir($h)) !== false) $rd[] = $e;
closedir($h);
$di = [];
foreach (new DirectoryIterator($d) as $f) $di[] = $f->getFilename();
$rdi = [];
foreach (new RecursiveIteratorIterator(new RecursiveDirectoryIterator($d, FilesystemIterator::SKIP_DOTS)) as $f) $rdi[] = $f->getFilename();
$none = scandir($d, SCANDIR_SORT_NONE);
$dir = dir($d);
$dd = [];
while (($e = $dir->read()) !== false) $dd[] = $e;
$dir->close();
var_dump($rd === $di, $rd === $none, $rd === $dd, array_values(array_diff($rd, ['.', '..'])) === $rdi);
$ls = @shell_exec('ls -f ' . escapeshellarg($d));
if (is_string($ls) && $ls !== '') {
    var_dump(explode("\n", trim($ls)) === $rd);
} else {
    var_dump(true);
}
$sorted = $rd;
sort($sorted);
var_dump($sorted === scandir($d));
foreach (scandir($d) as $n) if ($n[0] !== '.') unlink("$d/$n");
rmdir($d);

?>
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
