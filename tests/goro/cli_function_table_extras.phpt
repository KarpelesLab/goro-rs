--TEST--
php-cli's function table: no getallheaders()/apache_*_headers(), no clamp() (8.6), log2(), restore_include_path() (removed in 8.0), nor isset/empty
--FILE--
<?php
foreach (['getallheaders', 'apache_request_headers', 'apache_response_headers', 'clamp', 'log2', 'restore_include_path', 'isset', 'empty'] as $f) {
    echo $f, ': ', var_export(function_exists($f), true), ' ', var_export(in_array($f, get_defined_functions()['internal'], true), true), "\n";
}
try { log2(8); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $f = 'empty'; $f(0); } catch (Error $e) { echo $e->getMessage(), "\n"; }
?>
--EXPECT--
getallheaders: false false
apache_request_headers: false false
apache_response_headers: false false
clamp: false false
log2: false false
restore_include_path: false false
isset: false false
empty: false false
Call to undefined function log2()
Call to undefined function empty()
