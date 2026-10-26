--TEST--
Array literals don't fold installation-dependent constants (PHP_BINARY, PHP_OS, PHP_BINDIR) at compile time
--FILE--
<?php
$b = PHP_BINARY; $o = PHP_OS; $d = PHP_BINDIR; $v = PHP_VERSION;
var_dump([PHP_BINARY, '-n'][0] === $b, [PHP_OS][0] === $o, ['x' => PHP_BINDIR]['x'] === $d, [PHP_VERSION][0] === $v);
var_dump([PHP_INT_MAX, PHP_EOL, DIRECTORY_SEPARATOR]);
?>
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(true)
array(3) {
  [0]=>
  int(9223372036854775807)
  [1]=>
  string(1) "
"
  [2]=>
  string(1) "/"
}
