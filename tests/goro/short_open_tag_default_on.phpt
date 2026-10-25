--TEST--
short_open_tag defaults to "1" when no INI file sets it (php -n)
--FILE--
<?php
$php = getenv('TEST_PHP_EXECUTABLE');
$f = __DIR__ . '/short_open_tag_default_on.inc';
file_put_contents($f, "<?php var_dump(ini_get('short_open_tag')); ?>\n<? echo 'short tag ran', PHP_EOL;");
echo shell_exec(escapeshellarg($php) . ' -n ' . escapeshellarg($f));
echo shell_exec(escapeshellarg($php) . ' -n -d short_open_tag=0 ' . escapeshellarg($f));
?>
--CLEAN--
<?php
@unlink(__DIR__ . '/short_open_tag_default_on.inc');
?>
--EXPECT--
string(1) "1"
short tag ran
string(1) "0"
<? echo 'short tag ran', PHP_EOL;
