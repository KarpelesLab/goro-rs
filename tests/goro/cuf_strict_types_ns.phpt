--TEST--
A namespaced or dynamic call_user_func() call is checked as php-src compiles it
--FILE--
<?php
declare(strict_types=1);
namespace X;
try { var_dump(call_user_func('strlen', 5)); } catch (\TypeError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(\call_user_func('strlen', 5)); } catch (\TypeError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
function u(string $s) { return $s; }
try { var_dump(call_user_func('X\u', 5)); } catch (\TypeError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(\call_user_func('X\u', 5)); } catch (\TypeError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$f = 'call_user_func';
try { var_dump($f('strlen', 5)); } catch (\TypeError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(\call_user_func_array('strlen', [5])); } catch (\TypeError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECTF--
int(1)
TypeError: strlen(): Argument #1 ($string) must be of type string, int given
string(1) "5"
TypeError: X\u(): Argument #1 ($s) must be of type string, int given, called in %s on line 8
int(1)
TypeError: strlen(): Argument #1 ($string) must be of type string, int given
