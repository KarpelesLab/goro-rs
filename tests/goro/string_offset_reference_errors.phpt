--TEST--
Reference assignments to string offsets and arrays assigned to one
--FILE--
<?php
// Reference assignments to string offsets, and arrays assigned to one.
set_error_handler(function ($no, $str) { echo "E: $str\n"; return true; });
function t($f) { try { $f(); } catch (\Throwable $ex) { echo get_class($ex), ': ', $ex->getMessage(), "\n"; } }
t(function () { $a = "x"; $a[0] = &$b['k'][0]; var_dump($a, $b); });
t(function () { $e = "0"; $e['z'] = &$a; var_dump($e, $a); });
t(function () { $e = "10"; $e[] = &$a['k']; var_dump($e, $a); });
t(function () { $e = "10"; $e[0] = &$a; var_dump($e, $a); });
t(function () { $e = ""; $e[] = &$a; var_dump($e, $a); });
t(function () { $e = 5; $e[] = &$a; var_dump($e, $a); });
t(function () { $e = "ab"; $e[0][0] = &$a; var_dump($e, $a); });
t(function () { $e = "ab"; $x = &$e[0]; var_dump($e); });
t(function () { $e = "10"; $e[0] = [1, -7]; var_dump($e); });
t(function () { $f = "10"; $f[0] = new stdClass; var_dump($f); });
--EXPECT--
Error: Cannot create references to/from string offsets
TypeError: Cannot access offset of type string on string
Error: [] operator not supported for strings
Error: Cannot create references to/from string offsets
Error: [] operator not supported for strings
Error: Cannot use a scalar value as an array
Error: Cannot use string offset as an array
Error: Cannot create references to/from string offsets
E: Array to string conversion
E: Only the first byte will be assigned to the string offset
string(2) "A0"
Error: Object of class stdClass could not be converted to string
