--TEST--
opcache_is_script_cached() still finds a cached script whose file was removed
--INI--
opcache.enable=1
opcache.enable_cli=1
opcache.file_update_protection=0
opcache.validate_timestamps=0
--EXTENSIONS--
opcache
--FILE--
<?php
$key = sys_get_temp_dir() . '/opc_test_' . getmypid() . '.php';
file_put_contents($key, '<?php echo "loaded\n";');
var_dump(opcache_compile_file($key));
var_dump(opcache_is_script_cached($key));
unlink($key);
var_dump(opcache_is_script_cached($key));
var_dump(is_file($key));
include_once $key;
var_dump(opcache_invalidate($key, true));
var_dump(opcache_is_script_cached($key));
?>
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(false)
loaded
bool(true)
bool(false)
