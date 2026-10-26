--TEST--
Phar::mapPhar() from a script that only mentions __HALT_COMPILER(); in a string
--INI--
phar.readonly=0
--FILE--
<?php
// php-src looks for the __COMPILER_HALT_OFFSET__ constant of the executed
// file, not for the text "__HALT_COMPILER();" in it
$stub = "<?php __HALT_COMPILER(); ?>";
try {
    var_dump(Phar::mapPhar());
} catch (PharException $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
function f() { return "__halt_compiler();"; }
try {
    var_dump(Phar::mapPhar("x"));
} catch (PharException $e) {
    echo get_class($e), ": ", $e->getMessage(), "\n";
}
?>
--EXPECT--
PharException: __HALT_COMPILER(); must be declared in a phar
PharException: __HALT_COMPILER(); must be declared in a phar
