--TEST--
stream_resolve_include_path('./x') is relative to the cwd only; include opens 'missing/../x' like the plain-files wrapper (lexically)
--FILE--
<?php
$d = sys_get_temp_dir() . '/goro_srip_' . getmypid();
@mkdir("$d/a", 0777, true); @mkdir("$d/b", 0777, true);
file_put_contents("$d/same.php", '<?php return "d/same";');
file_put_contents("$d/a/same.php", '<?php return "a/same";');
file_put_contents("$d/b/only_b.php", '<?php return "only_b";');
chdir($d);
set_include_path('a');
var_dump(basename(dirname(stream_resolve_include_path('same.php'))));
var_dump(basename(dirname(stream_resolve_include_path('./same.php'))));
var_dump(stream_resolve_include_path('./nope.php'));
var_dump(include 'b/only_b.php');
var_dump(include_once 'B/../b/only_b.php');
var_dump(include 'zz/../same.php');
foreach (["$d/same.php", "$d/a/same.php", "$d/b/only_b.php"] as $f) unlink($f);
rmdir("$d/a"); rmdir("$d/b"); rmdir($d);
--EXPECTF--
string(1) "a"
string(%d) "goro_srip_%d"
bool(false)
string(6) "only_b"
bool(true)
string(6) "d/same"
