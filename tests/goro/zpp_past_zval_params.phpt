--TEST--
zpp checks the parameters after one parsed as a zval or resource; strtr(), array_replace(), get_class_vars(), setcookie(), htmlentities()
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
$m = fopen('php://memory', 'w+');
t(fn() => fprintf('x', 'a'));
t(fn() => fprintf($m, []));
t(fn() => fprintf($m, '%s', []));
t(fn() => vfprintf($m, '%s', 'x'));
t(fn() => flock('x', 1));
t(fn() => flock($m, 'z'));
t(fn() => ftruncate($m, 'z'));
t(fn() => ftruncate($m, null));
t(fn() => fscanf($m, []));
t(fn() => stream_set_blocking($m, []));
t(fn() => stream_set_timeout($m, 'x'));
t(fn() => stream_get_line($m, 'x'));
t(fn() => stream_copy_to_stream($m, $m, 'x'));
t(fn() => array_key_exists([], []));
t(fn() => array_key_exists('a', 'b'));
t(fn() => key_exists(1, null));
t(fn() => method_exists(1, 'a'));
t(fn() => method_exists('stdClass', null));
t(fn() => property_exists(new stdClass, 1));
t(fn() => count(new stdClass, []));
t(fn() => count('a', 1.5));
$a = [3, 1];
t(fn() => sort($a, 'x'));
t(fn() => ksort($a, []));
t(fn() => file_get_contents('/nonexist', false, null, 'x'));
t(fn() => file_get_contents('/nonexist', [], null));
t(fn() => file_get_contents('/nonexist', false, 1));
t(fn() => stream_context_set_params(1, []));
t(fn() => stream_get_contents($m, 'x'));
t(fn() => setcookie('a', 'b', 0, []));
t(fn() => setcookie());
t(fn() => htmlspecialchars('a', null));
t(fn() => strtr([], 'a', 'b'));
t(fn() => strtr(new stdClass, 'a', 'b'));
t(fn() => strtr('abc', []));
t(fn() => strtr('abc', 'a', []));
t(fn() => strtr('abc', new stdClass));
t(fn() => array_replace('zz', []));
t(fn() => array_replace([], 'zz'));
t(fn() => array_replace_recursive(new stdClass, []));
t(fn() => class_implements('stdClass', []));
t(fn() => class_parents('stdClass', []));
t(fn() => class_uses('stdClass', []));
t(fn() => count([], 'zz'));
t(fn() => sizeof([], 'zz'));
$f = 'count'; t(fn() => $f([], 'zz'));
t(fn() => htmlentities('a', []));
t(fn() => htmlentities('a', ENT_QUOTES, []));
t(fn() => htmlentities('a', ENT_QUOTES, 'UTF-8', []));
t(fn() => htmlspecialchars('a', new stdClass));
t(fn() => htmlspecialchars('a', ENT_QUOTES, new stdClass));
t(fn() => htmlspecialchars('a', ENT_QUOTES, 'UTF-8', new stdClass));
t(fn() => method_exists('stdClass', []));
t(fn() => property_exists('stdClass', []));
t(fn() => method_exists([], 'a'));
t(fn() => property_exists([], 'a'));
t(fn() => set_file_buffer(STDOUT, 'zz'));
t(fn() => setrawcookie([]));
t(fn() => get_class_vars(new stdClass));
t(fn() => get_class_vars([]));
t(fn() => strtr('abc', null, 'x'));
t(fn() => strtr('abc', 'a', null));
t(fn() => strtr(null, 'a', 'b'));
t(fn() => strtr(null, []));
t(fn() => strtr('abc', null));
t(fn() => strtr('abc', 1, 2));
t(fn() => strtr(12, ['1' => 'x']));
t(fn() => strtr('abc', 'a', 1.5));
t(fn() => strtr('abc', 'a', new stdClass));
t(fn() => strtr('abc'));
t(fn() => strtr('abc', 'a', 'b', 'c'));
t(fn() => strtr([], []));
t(fn() => strtr('abc', [], 'x'));
$f = fopen('php://memory', 'r');
t(fn() => strtr('abc', $f, 'x'));
t(fn() => array_replace([], 1, []));
t(fn() => array_replace());
t(fn() => array_replace_recursive([], [], null));
t(fn() => get_class_vars(new stdClass));
t(fn() => get_class_vars([]));
t(fn() => get_class_vars(null));
t(fn() => get_class_vars(1));
t(fn() => get_class_vars('Nope'));
--EXPECTF--
TypeError: fprintf(): Argument #1 ($stream) must be of type resource, string given
TypeError: fprintf(): Argument #2 ($format) must be of type string, array given

Warning: Array to string conversion in %s on line 6
int(5)
TypeError: vfprintf(): Argument #3 ($values) must be of type array, string given
TypeError: flock(): Argument #1 ($stream) must be of type resource, string given
TypeError: flock(): Argument #2 ($operation) must be of type int, string given
TypeError: ftruncate(): Argument #2 ($size) must be of type int, string given

Deprecated: ftruncate(): Passing null to parameter #2 ($size) of type int is deprecated in %s on line 11
bool(true)
TypeError: fscanf(): Argument #2 ($format) must be of type string, array given
TypeError: stream_set_blocking(): Argument #2 ($enable) must be of type bool, array given
TypeError: stream_set_timeout(): Argument #2 ($seconds) must be of type int, string given
TypeError: stream_get_line(): Argument #2 ($length) must be of type int, string given
TypeError: stream_copy_to_stream(): Argument #3 ($length) must be of type ?int, string given
TypeError: Cannot access offset of type array on array
TypeError: array_key_exists(): Argument #2 ($array) must be of type array, string given
TypeError: key_exists(): Argument #2 ($array) must be of type array, null given
TypeError: method_exists(): Argument #1 ($object_or_class) must be of type object|string, int given

Deprecated: method_exists(): Passing null to parameter #2 ($method) of type string is deprecated in %s on line 21
bool(false)
bool(false)
TypeError: count(): Argument #2 ($mode) must be of type int, array given

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 24
TypeError: count(): Argument #1 ($value) must be of type Countable|array, string given
TypeError: sort(): Argument #2 ($flags) must be of type int, string given
TypeError: ksort(): Argument #2 ($flags) must be of type int, array given
TypeError: file_get_contents(): Argument #4 ($offset) must be of type int, string given
TypeError: file_get_contents(): Argument #2 ($use_include_path) must be of type bool, array given
TypeError: file_get_contents(): Argument #3 ($context) must be of type resource or null, int given
TypeError: stream_context_set_params(): Argument #1 ($context) must be of type resource, int given
TypeError: stream_get_contents(): Argument #2 ($length) must be of type ?int, string given
TypeError: setcookie(): Argument #4 ($path) must be of type string, array given
ArgumentCountError: setcookie() expects at least 1 argument, 0 given

Deprecated: htmlspecialchars(): Passing null to parameter #2 ($flags) of type int is deprecated in %s on line 35
string(1) "a"
TypeError: strtr(): Argument #1 ($string) must be of type string, array given
TypeError: strtr(): Argument #1 ($string) must be of type string, stdClass given
string(3) "abc"
TypeError: strtr(): Argument #3 ($to) must be of type string, array given
TypeError: strtr(): Argument #2 ($from) must be of type array, stdClass given
TypeError: array_replace(): Argument #1 ($array) must be of type array, string given
TypeError: array_replace(): Argument #2 must be of type array, string given
TypeError: array_replace_recursive(): Argument #1 ($array) must be of type array, stdClass given
TypeError: class_implements(): Argument #2 ($autoload) must be of type bool, array given
TypeError: class_parents(): Argument #2 ($autoload) must be of type bool, array given
TypeError: class_uses(): Argument #2 ($autoload) must be of type bool, array given
TypeError: count(): Argument #2 ($mode) must be of type int, string given
TypeError: sizeof(): Argument #2 ($mode) must be of type int, string given
TypeError: count(): Argument #2 ($mode) must be of type int, string given
TypeError: htmlentities(): Argument #2 ($flags) must be of type int, array given
TypeError: htmlentities(): Argument #3 ($encoding) must be of type ?string, array given
TypeError: htmlentities(): Argument #4 ($double_encode) must be of type bool, array given
TypeError: htmlspecialchars(): Argument #2 ($flags) must be of type int, stdClass given
TypeError: htmlspecialchars(): Argument #3 ($encoding) must be of type ?string, stdClass given
TypeError: htmlspecialchars(): Argument #4 ($double_encode) must be of type bool, stdClass given
TypeError: method_exists(): Argument #2 ($method) must be of type string, array given
TypeError: property_exists(): Argument #2 ($property) must be of type string, array given
TypeError: method_exists(): Argument #1 ($object_or_class) must be of type object|string, array given
TypeError: property_exists(): Argument #1 ($object_or_class) must be of type object|string, array given
TypeError: set_file_buffer(): Argument #2 ($size) must be of type int, string given
TypeError: setrawcookie(): Argument #1 ($name) must be of type string, array given
Error: Object of class stdClass could not be converted to string

Warning: Array to string conversion in %s on line 63
TypeError: get_class_vars(): Argument #1 ($class) must be a valid class name, Array given

Deprecated: strtr(): Passing null to parameter #2 ($from) of type array|string is deprecated in %s on line 64
string(3) "abc"

Deprecated: strtr(): Passing null to parameter #3 ($to) of type ?string is deprecated in %s on line 65
string(3) "abc"

Deprecated: strtr(): Passing null to parameter #1 ($string) of type string is deprecated in %s on line 66
string(0) ""

Deprecated: strtr(): Passing null to parameter #1 ($string) of type string is deprecated in %s on line 67
string(0) ""
TypeError: strtr(): Argument #2 ($from) must be of type array, null given
string(3) "abc"
string(2) "x2"
string(3) "1bc"
TypeError: strtr(): Argument #3 ($to) must be of type string, stdClass given
ArgumentCountError: strtr() expects exactly 2 arguments, 1 given
ArgumentCountError: strtr() expects exactly 3 arguments, 4 given
TypeError: strtr(): Argument #1 ($string) must be of type string, array given
TypeError: strtr(): Argument #2 ($from) must be of type string, array given
TypeError: strtr(): Argument #2 ($from) must be of type string, resource given
TypeError: array_replace(): Argument #2 must be of type array, int given
ArgumentCountError: array_replace() expects at least 1 argument, 0 given
TypeError: array_replace_recursive(): Argument #3 must be of type array, null given
Error: Object of class stdClass could not be converted to string

Warning: Array to string conversion in %s on line 83
TypeError: get_class_vars(): Argument #1 ($class) must be a valid class name, Array given
TypeError: get_class_vars(): Argument #1 ($class) must be a valid class name,  given
TypeError: get_class_vars(): Argument #1 ($class) must be a valid class name, 1 given
TypeError: get_class_vars(): Argument #1 ($class) must be a valid class name, Nope given
