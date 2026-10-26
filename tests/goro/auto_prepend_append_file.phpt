--TEST--
auto_prepend_file / auto_append_file run around the main script (exit() skips the append; a missing prepend file is fatal)
--FILE--
<?php
$d = sys_get_temp_dir() . '/goro_auto_' . getmypid();
@mkdir($d);
file_put_contents("$d/pre.php", '<?php function f() { return basename(__FILE__) . ":" . __LINE__; } $x = 5; echo "PRE ", basename(__FILE__), " ", count(get_included_files()), "\n";');
file_put_contents("$d/app.php", '<?php echo "APPEND ", $x, "\n";');
file_put_contents("$d/main.php", '<?php echo f(), " ", $x, " ", implode(",", array_map("basename", get_included_files())), "\n";');
file_put_contents("$d/exit.php", '<?php echo "MAIN\n"; exit(3);');
function run($args) {
    $p = proc_open(array_merge([PHP_BINARY, '-n', '-d', 'display_errors=1'], $args), [1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
    $out = stream_get_contents($pipes[1]) . stream_get_contents($pipes[2]);
    echo $out, "rc=", proc_close($p), "\n";
}
run(['-d', "auto_prepend_file=$d/pre.php", '-d', "auto_append_file=$d/app.php", "$d/main.php"]);
run(['-d', "auto_prepend_file=$d/pre.php", '-d', "auto_append_file=$d/app.php", "$d/exit.php"]);
run(['-d', "auto_prepend_file=$d/pre.php", '-r', 'echo "code\n";']);
run(['-d', "auto_prepend_file=$d/nope.php", "$d/main.php"]);
foreach (['pre', 'app', 'main', 'exit'] as $f) unlink("$d/$f.php");
rmdir($d);
?>
--EXPECTF--
PRE pre.php 2
pre.php:1 5 main.php,pre.php
APPEND 5
rc=0
PRE pre.php 2
MAIN
rc=3
code
rc=0

Warning: Unknown: Failed to open stream: No such file or directory in Unknown on line 0

Fatal error: Failed opening required '%snope.php' (include_path='%s') in Unknown on line 0
Stack trace:
#0 {main}
rc=255
