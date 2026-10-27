--TEST--
File names that are not UTF-8: the filesystem functions, directory reading, SPL file classes, include and their diagnostics keep the raw bytes
--FILE--
<?php
$d = sys_get_temp_dir() . "/goro_u8p_" . getmypid() . "/";
@mkdir($d);
set_error_handler(function ($no, $msg) use ($d) {
    echo "Warning: ", esc(str_replace($d, "D/", $msg)), "\n";
    return true;
});
// Non-ASCII bytes shown as \xHH.
function esc($s) {
    return preg_replace_callback('/[\x80-\xff]/', fn($m) => sprintf('\\x%02x', ord($m[0])), $s);
}
function hx($v) { return is_string($v) ? esc($v) : $v; }
$f = $d . "a\xffb.txt";
var_dump(file_put_contents($f, "x"));
var_dump(hx(implode(",", array_diff(scandir($d), [".", ".."]))));
$h = opendir($d);
while (($e = readdir($h)) !== false) if ($e[0] != '.') var_dump(hx($e));
closedir($h);
$dd = dir($d);
while (($e = $dd->read()) !== false) if ($e[0] != '.') var_dump(hx($e));
$dd->close();
var_dump(file_exists($f), is_file($f), is_readable($f), is_writable($f), filesize($f), file_get_contents($f), file($f));
var_dump(hx(basename(realpath($f))), fileperms($f) & 0777 ? true : false, is_link($f), filetype($f), stat($f)['size'], lstat($f)['size']);
var_dump(touch($f), chmod($f, 0644));
readfile($f); echo "\n";
var_dump(hx(basename(glob($d . "*")[0])));
var_dump(copy($f, $d . "c\xfe"), rename($d . "c\xfe", $d . "r\xfd"), file_exists($d . "r\xfd"), unlink($d . "r\xfd"));
var_dump(symlink($f, $d . "l\xfc"), hx(basename(readlink($d . "l\xfc"))), unlink($d . "l\xfc"));
var_dump(link($f, $d . "h\xfb"), unlink($d . "h\xfb"));
$cwd = getcwd();
var_dump(mkdir($d . "s\xfa"), is_dir($d . "s\xfa"), chdir($d . "s\xfa"), hx(basename(getcwd())), chdir($cwd), rmdir($d . "s\xfa"));
$fp = fopen($f, "r"); var_dump(hx(basename(stream_get_meta_data($fp)['uri']))); fclose($fp);
var_dump(file_get_contents("file://" . $f));
foreach (new DirectoryIterator($d) as $e) if (!$e->isDot()) var_dump(hx($e->getFilename()), $e->getSize());
foreach (new FilesystemIterator($d) as $p => $e) var_dump(hx(basename($p)));
foreach (new GlobIterator($d . "*") as $p => $e) var_dump(hx(basename($p)));
$o = new SplFileObject($f); var_dump($o->fgets(), $o->getSize(), hx(basename($o->getRealPath())));
$o = null;
file_put_contents($d . "i\xf8.ini", "a=1\n"); var_dump(parse_ini_file($d . "i\xf8.ini")); unlink($d . "i\xf8.ini");
file_put_contents($d . "inc\xf7.php", "<?php return basename(__FILE__) . ' ' . (__DIR__ . '/' === \$GLOBALS['d'] ? 'dir ok' : 'dir bad');");
var_dump(hx(include $d . "inc\xf7.php"));
unlink($d . "inc\xf7.php");
$n = $d . "no\xfe";
fopen($n, "r");
fopen("file://" . $n, "r");
file_get_contents($n);
file_put_contents($n . "/x", "");
unlink($n);
rmdir($n);
stat($n);
filesize($n);
filetype($n);
opendir($n);
touch($n . "/x");
copy($n, $d . "z");
rename($n, $d . "z\xfd");
readfile($n);
parse_ini_file($n);
try { new SplFileObject($n); } catch (Exception $e) { echo esc(str_replace($d, "D/", $e->getMessage())), "\n"; }
try { new DirectoryIterator($n); } catch (Exception $e) { echo esc(str_replace($d, "D/", $e->getMessage())), "\n"; }
try { (new SplFileInfo($n))->getSize(); } catch (Exception $e) { echo esc(str_replace($d, "D/", $e->getMessage())), "\n"; }
var_dump(unlink($f), rmdir($d));
?>
--EXPECT--
int(1)
string(10) "a\xffb.txt"
string(10) "a\xffb.txt"
string(10) "a\xffb.txt"
bool(true)
bool(true)
bool(true)
bool(true)
int(1)
string(1) "x"
array(1) {
  [0]=>
  string(1) "x"
}
string(10) "a\xffb.txt"
bool(true)
bool(false)
string(4) "file"
int(1)
int(1)
bool(true)
bool(true)
x
string(10) "a\xffb.txt"
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
string(10) "a\xffb.txt"
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
string(5) "s\xfa"
bool(true)
bool(true)
string(10) "a\xffb.txt"
string(1) "x"
string(10) "a\xffb.txt"
int(1)
string(10) "a\xffb.txt"
string(10) "a\xffb.txt"
string(1) "x"
int(1)
string(10) "a\xffb.txt"
array(1) {
  ["a"]=>
  string(1) "1"
}
string(18) "inc\xf7.php dir ok"
Warning: fopen(D/no\xfe): Failed to open stream: No such file or directory
Warning: fopen(file://D/no\xfe): Failed to open stream: No such file or directory
Warning: file_get_contents(D/no\xfe): Failed to open stream: No such file or directory
Warning: file_put_contents(D/no\xfe/x): Failed to open stream: No such file or directory
Warning: unlink(D/no\xfe): No such file or directory
Warning: rmdir(D/no\xfe): No such file or directory
Warning: stat(): stat failed for D/no\xfe
Warning: filesize(): stat failed for D/no\xfe
Warning: filetype(): Lstat failed for D/no\xfe
Warning: opendir(D/no\xfe): Failed to open directory: No such file or directory
Warning: touch(): Unable to create file D/no\xfe/x because No such file or directory
Warning: copy(D/no\xfe): Failed to open stream: No such file or directory
Warning: rename(D/no\xfe,D/z\xfd): No such file or directory
Warning: readfile(D/no\xfe): Failed to open stream: No such file or directory
Warning: parse_ini_file(D/no\xfe): Failed to open stream: No such file or directory
SplFileObject::__construct(D/no\xfe): Failed to open stream: No such file or directory
DirectoryIterator::__construct(D/no\xfe): Failed to open directory: No such file or directory
SplFileInfo::getSize(): stat failed for D/no\xfe
bool(true)
bool(true)
