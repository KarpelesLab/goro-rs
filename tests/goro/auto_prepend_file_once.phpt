--TEST--
auto_prepend_file counts as included for include_once / require_once (and so does the main script)
--FILE--
<?php
$d = sys_get_temp_dir() . '/goro_autoonce_' . getmypid();
@mkdir($d);
file_put_contents("$d/inc.inc", '<?php echo "Included!\n";');
file_put_contents("$d/main.php", '<?php include_once(__DIR__ . "/inc.inc"); require_once(__DIR__ . "/inc.inc"); var_dump(include_once(__FILE__)); echo count(get_included_files()), "\n";');
$p = proc_open([PHP_BINARY, '-n', '-d', "include_path=$d", '-d', 'auto_prepend_file=inc.inc', "$d/main.php"], [1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
echo stream_get_contents($pipes[1]), stream_get_contents($pipes[2]), "rc=", proc_close($p), "\n";
unlink("$d/inc.inc"); unlink("$d/main.php"); rmdir($d);
?>
--EXPECT--
Included!
bool(true)
2
rc=0
