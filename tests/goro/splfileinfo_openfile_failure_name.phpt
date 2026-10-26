--TEST--
SplFileInfo::openFile(): a file that cannot be opened is reported under openFile()
--FILE--
<?php
$i = new SplFileInfo(sys_get_temp_dir() . "/nonexist_" . getmypid());
try { $i->openFile("r"); } catch (RuntimeException $e) { echo str_replace(sys_get_temp_dir(), "TMP", $e->getMessage()), "\n"; }
?>
--EXPECTF--
SplFileInfo::openFile(TMP/nonexist_%d): Failed to open stream: No such file or directory
