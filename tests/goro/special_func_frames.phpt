--TEST--
Frames of the builtins php-src compiles to opcodes (and of their dynamic calls)
--FILE--
<?php
class T { function __toString(): string { throw new Exception("ts"); } }
function tr($f) { try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), " | "; foreach ($e->getTrace() as $fr) echo $fr['function'], ","; echo "\n"; } }
tr(function() { $f = 'strlen'; $f(new T); });
tr(function() { $f = 'sprintf'; $f('%s', new T); });
tr(function() { $fmt = '%s'; sprintf($fmt, new T); });
tr(function() { $h = ["a"]; in_array(new T, $h); });
tr(function() { in_array(new T, ["a"]); });
tr(function() { in_array(new T, ["a"], false); });
tr(function() { $f = 'in_array'; $f(new T, ["a"]); });
tr(function() { call_user_func('strlen', new T); });
tr(function() { array_map('strval', [new T]); });
tr(fn() => strlen(new T));
tr(fn() => strval(new T));
tr(fn() => intval(new T));
tr(fn() => ord(new T));
tr(fn() => chr(new T));
tr(fn() => array_key_exists(new T, []));
tr(fn() => array_slice(func_get_args(), 1));
tr(fn() => gettype(new T));
tr(fn() => get_class(new T));
tr(fn() => is_int(new T));
tr(fn() => count(new T));
tr(fn() => in_array(new T, ["a"]));
tr(fn() => sprintf("%s", new T));
tr(fn() => boolval(new T));
tr(fn() => floatval(new T));
var_dump(strlen(null), intval(null), strval(null), boolval(null), ord(""), bin2hex(chr(-1)), bin2hex(chr(256)));
var_dump(intval("12", 16), intval([]), intval([0]), floatval([]));
try { var_dump(intval(new stdClass)); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { var_dump(floatval(new stdClass)); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { var_dump(strval([])); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { var_dump(strlen([])); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { var_dump(count(5)); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { var_dump(ord([])); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { var_dump(chr("a")); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { var_dump(array_key_exists([], [])); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
var_dump(array_key_exists(null, ["" => 1]), array_key_exists(1.5, [1 => 1]), array_key_exists(true, [1 => 1]));
try { var_dump(get_class()); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
Exception: ts | __toString,strlen,{closure:%s:4},tr,
Exception: ts | __toString,sprintf,{closure:%s:5},tr,
Exception: ts | __toString,sprintf,{closure:%s:6},tr,
Exception: ts | __toString,in_array,{closure:%s:7},tr,
Exception: ts | __toString,{closure:%s:8},tr,
Exception: ts | __toString,{closure:%s:9},tr,
Exception: ts | __toString,in_array,{closure:%s:10},tr,
Exception: ts | __toString,strlen,{closure:%s:11},tr,
Exception: ts | __toString,strval,array_map,{closure:%s:12},tr,
Exception: ts | __toString,{closure:%s:13},tr,
Exception: ts | __toString,{closure:%s:14},tr,

Warning: Object of class T could not be converted to int in %s on line 15
Exception: ts | __toString,ord,{closure:%s:16},tr,
TypeError: chr(): Argument #1 ($codepoint) must be of type int, T given | chr,{closure:%s:17},tr,
TypeError: Cannot access offset of type T on array | {closure:%s:18},tr,
TypeError: count(): Argument #1 ($value) must be of type Countable|array, T given | {closure:%s:23},tr,
Exception: ts | __toString,{closure:%s:24},tr,
Exception: ts | __toString,{closure:%s:25},tr,

Warning: Object of class T could not be converted to float in %s on line 27

Deprecated: strlen(): Passing null to parameter #1 ($string) of type string is deprecated in %s on line 28

Deprecated: ord(): Providing an empty string is deprecated in %s on line 28

Deprecated: chr(): Providing a value not in-between 0 and 255 is deprecated, this is because a byte value must be in the [0, 255] interval. The value used will be constrained using % 256 in %s on line 28

Deprecated: chr(): Providing a value not in-between 0 and 255 is deprecated, this is because a byte value must be in the [0, 255] interval. The value used will be constrained using % 256 in %s on line 28
int(0)
int(0)
string(0) ""
bool(false)
int(0)
string(2) "ff"
string(2) "00"
int(18)
int(0)
int(1)
float(0)

Warning: Object of class stdClass could not be converted to int in %s on line 30
int(1)

Warning: Object of class stdClass could not be converted to float in %s on line 31
float(1)

Warning: Array to string conversion in %s on line 32
string(5) "Array"
strlen(): Argument #1 ($string) must be of type string, array given
count(): Argument #1 ($value) must be of type Countable|array, int given
ord(): Argument #1 ($character) must be of type string, array given
chr(): Argument #1 ($codepoint) must be of type int, string given
Cannot access offset of type array on array

Deprecated: Using null as the key parameter for array_key_exists() is deprecated, use an empty string instead in %s on line 38

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 38
bool(true)
bool(true)
bool(true)
get_class() without arguments must be called from within a class
