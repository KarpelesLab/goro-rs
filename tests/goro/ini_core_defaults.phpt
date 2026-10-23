--TEST--
Core INI directives have their default values; system ones cannot be set at run time
--FILE--
<?php
foreach (['upload_max_filesize', 'post_max_size', 'file_uploads', 'max_input_vars', 'max_file_uploads',
          'allow_url_fopen', 'default_socket_timeout', 'variables_order', 'enable_post_data_reading',
          'default_mimetype', 'unserialize_max_depth', 'realpath_cache_size'] as $k) {
    echo $k, '=', var_export(ini_get($k), true), "\n";
}
// PHP_INI_SYSTEM / PHP_INI_PERDIR entries cannot be changed at run time
var_dump(ini_set('upload_max_filesize', '64M'), ini_get('upload_max_filesize'));
var_dump(ini_set('allow_url_fopen', '0'), ini_get('allow_url_fopen'));
var_dump(ini_set('default_socket_timeout', '5'), ini_get('default_socket_timeout'));
?>
--EXPECT--
upload_max_filesize='2M'
post_max_size='8M'
file_uploads='1'
max_input_vars='1000'
max_file_uploads='20'
allow_url_fopen='1'
default_socket_timeout='60'
variables_order='EGPCS'
enable_post_data_reading='1'
default_mimetype='text/html'
unserialize_max_depth='4096'
realpath_cache_size='4096K'
bool(false)
string(2) "2M"
bool(false)
string(1) "1"
string(2) "60"
string(1) "5"
