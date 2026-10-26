--TEST--
User constants named like INI entries (my.const, precision) live in their own table
--FILE--
<?php
namespace Foo {
const bar = 3;
var_dump(\constant("Foo\\bar"), bar, defined("foo\\bar"));
}
namespace {
define("my.const", 5); var_dump(constant("my.const"));
var_dump(ini_get("my.const"));
var_dump(define("my.const", 6));
define("precision", 7); var_dump(constant("precision")); var_dump(ini_get("precision"));
echo 1.123456789012345, "\n";
define("Foo.Bar", 1); var_dump(constant("Foo.Bar"));
var_dump(defined("my.const"), defined("precision"), defined("date.timezone"), defined("memory_limit"));
$c = get_defined_constants(true)["user"];
var_dump($c);
var_dump(ini_set("my.const", 3));
var_dump((new ReflectionConstant("my.const"))->getValue());
const display_errors = 9;
var_dump(display_errors, ini_get("display_errors"));
function f($x = display_errors) { return $x; }
var_dump(f());
}
--EXPECTF--
int(3)
int(3)
bool(true)
int(5)
bool(false)

Warning: Constant my.const already defined, this will be an error in PHP 9 in %s on line 9
bool(false)
int(7)
string(2) "14"
1.1234567890123
int(1)
bool(true)
bool(true)
bool(false)
bool(false)
array(4) {
  ["Foo\bar"]=>
  int(3)
  ["my.const"]=>
  int(5)
  ["precision"]=>
  int(7)
  ["Foo.Bar"]=>
  int(1)
}
bool(false)
int(5)
int(9)
string(1) "1"
int(9)
