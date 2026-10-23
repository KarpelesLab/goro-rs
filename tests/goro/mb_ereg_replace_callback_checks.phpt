--TEST--
mb_ereg_replace_callback(): callback validation, array results warn
--EXTENSIONS--
mbstring
--FILE--
<?php
try {
    mb_ereg_replace_callback('a', 'nonexist', 'a');
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
var_dump(mb_ereg_replace_callback('a', fn($m) => [1], 'xa'));
--EXPECTF--
mb_ereg_replace_callback(): Argument #2 ($callback) must be a valid callback, function "nonexist" not found or invalid function name

Warning: Array to string conversion in %s on line 7
string(6) "xArray"
