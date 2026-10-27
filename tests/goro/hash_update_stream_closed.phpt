--TEST--
hash_update_stream() rejects a closed stream or another resource type itself, while parsing its parameters
--EXTENSIONS--
hash
--FILE--
<?php
$fp = fopen("php://memory", "r");
fclose($fp);
try { hash_update_stream(hash_init('md5'), $fp); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { hash_update_stream(hash_init('md5'), $fp, "x"); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { hash_update_stream(hash_init('md5'), stream_context_create()); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$fp = fopen("data:,stream", "r");
$c = hash_init('md5');
var_dump(hash_update_stream($c, $fp, 3), hash_final($c) === md5("str"));
?>
--EXPECT--
TypeError: hash_update_stream(): Argument #2 ($stream) must be an open stream resource
TypeError: hash_update_stream(): Argument #2 ($stream) must be an open stream resource
TypeError: hash_update_stream(): Argument #2 ($stream) must be an open stream resource
int(3)
bool(true)
