--TEST--
gzopen() invalid mode warning shows the mode's raw bytes
--FILE--
<?php
$f = gzopen(__DIR__ . '/gzopen_invalid_mode_raw_bytes.gz', "\xff\x01");
var_dump($f);
$f = gzopen(__DIR__ . '/gzopen_invalid_mode_raw_bytes.gz', "w\xff");
var_dump($f);
gzclose($f);
?>
--CLEAN--
<?php
@unlink(__DIR__ . '/gzopen_invalid_mode_raw_bytes.gz');
?>
--EXPECTF--

Warning: gzopen(%s): Failed to open stream: `ÿ' is not a valid mode for fopen in %s on line 2
bool(false)
resource(%d) of type (stream)
