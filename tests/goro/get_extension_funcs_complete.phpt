--TEST--
get_extension_funcs() lists every function of pcntl, ftp, mbstring regex, mhash and opcache; PDO driver pseudo-classes are not classes
--EXTENSIONS--
pcntl
ftp
mbstring
pdo_sqlite
--FILE--
<?php
$p = get_extension_funcs('pcntl');
var_dump(in_array('pcntl_fork', $p), in_array('pcntl_waitpid', $p), in_array('pcntl_signal', $p), in_array('pcntl_alarm', $p));
$f = get_extension_funcs('ftp');
var_dump($f[0], in_array('ftp_ssl_connect', $f), in_array('ftp_get_option', $f));
$m = get_extension_funcs('mbstring');
var_dump(in_array('mb_ereg', $m), in_array('mb_split', $m), in_array('mb_regex_set_options', $m));
var_dump(in_array('mhash', get_extension_funcs('hash')));
var_dump(in_array('opcache_get_status', get_extension_funcs('Zend OPcache') ?: []));
foreach (get_defined_functions()['internal'] as $fn) {
    $e = (new ReflectionFunction($fn))->getExtensionName();
    if ($e !== false && !in_array($fn, array_map("strtolower", get_extension_funcs($e) ?: []), true)) echo "not listed: $fn ($e)\n";
}
var_dump(class_exists('PDO_SQLite_Ext'), in_array('PDO_SQLite_Ext', (new ReflectionExtension('pdo_sqlite'))->getClassNames()));
?>
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(true)
string(11) "ftp_connect"
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(false)
bool(false)
