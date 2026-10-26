--TEST--
ReflectionConstant::getName() is the registered name (no leading backslash, TRUE); the name property keeps the given one
--FILE--
<?php
namespace Foo;
function t($f) { try { $r = $f(); var_dump($r); } catch (\Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; } }

const A = 1;
/** d */ const B = [1];
define('Foo\DEF', 2);
define('LOWER', 3);
foreach (['Foo\A', 'Foo\B', 'Foo\DEF', 'LOWER', 'PHP_VERSION', 'E_ALL', 'M_PI', 'PHP_EOL', 'JSON_ERROR_NONE', '\Foo\A', 'true'] as $n) {
  try { $r = new \ReflectionConstant($n); } catch (\Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; continue; }
  var_dump($r->getName(), $r->getNamespaceName(), $r->getShortName(), $r->getValue(), $r->isDeprecated());
  var_dump($r->getExtension()?->getName(), $r->getExtensionName(), $r->getFileName());
  echo $r;
  var_dump($r);
}
t(fn() => new \ReflectionConstant('NOPE'));
t(fn() => new \ReflectionConstant('E_STRICT'));
echo new \ReflectionConstant('E_STRICT');
var_dump((new \ReflectionConstant('E_STRICT'))->isDeprecated());
?>
--EXPECTF--
string(5) "Foo\A"
string(3) "Foo"
string(1) "A"
int(1)
bool(false)
NULL
bool(false)
string(%d) "%s"
Constant [ int Foo\A ] { 1 }
object(ReflectionConstant)#1 (1) {
  ["name"]=>
  string(5) "Foo\A"
}
string(5) "Foo\B"
string(3) "Foo"
string(1) "B"
array(1) {
  [0]=>
  int(1)
}
bool(false)
NULL
bool(false)
string(%d) "%s"
Constant [ array Foo\B ] { Array }
object(ReflectionConstant)#2 (1) {
  ["name"]=>
  string(5) "Foo\B"
}
string(7) "Foo\DEF"
string(3) "Foo"
string(3) "DEF"
int(2)
bool(false)
NULL
bool(false)
string(%d) "%s"
Constant [ int Foo\DEF ] { 2 }
object(ReflectionConstant)#1 (1) {
  ["name"]=>
  string(7) "Foo\DEF"
}
string(5) "LOWER"
string(0) ""
string(5) "LOWER"
int(3)
bool(false)
NULL
bool(false)
string(%d) "%s"
Constant [ int LOWER ] { 3 }
object(ReflectionConstant)#2 (1) {
  ["name"]=>
  string(5) "LOWER"
}
string(11) "PHP_VERSION"
string(0) ""
string(11) "PHP_VERSION"
string(5) "8.5.4"
bool(false)
string(4) "Core"
string(4) "Core"
bool(false)
Constant [ <persistent> string PHP_VERSION ] { 8.5.4 }
object(ReflectionConstant)#1 (1) {
  ["name"]=>
  string(11) "PHP_VERSION"
}
string(5) "E_ALL"
string(0) ""
string(5) "E_ALL"
int(30719)
bool(false)
string(4) "Core"
string(4) "Core"
bool(false)
Constant [ <persistent> int E_ALL ] { 30719 }
object(ReflectionConstant)#2 (1) {
  ["name"]=>
  string(5) "E_ALL"
}
string(4) "M_PI"
string(0) ""
string(4) "M_PI"
float(3.141592653589793)
bool(false)
string(8) "standard"
string(8) "standard"
bool(false)
Constant [ <persistent> float M_PI ] { 3.1415926535898 }
object(ReflectionConstant)#1 (1) {
  ["name"]=>
  string(4) "M_PI"
}
string(7) "PHP_EOL"
string(0) ""
string(7) "PHP_EOL"
string(1) "
"
bool(false)
string(4) "Core"
string(4) "Core"
bool(false)
Constant [ <persistent> string PHP_EOL ] { 
 }
object(ReflectionConstant)#2 (1) {
  ["name"]=>
  string(7) "PHP_EOL"
}
string(15) "JSON_ERROR_NONE"
string(0) ""
string(15) "JSON_ERROR_NONE"
int(0)
bool(false)
string(4) "json"
string(4) "json"
bool(false)
Constant [ <persistent> int JSON_ERROR_NONE ] { 0 }
object(ReflectionConstant)#1 (1) {
  ["name"]=>
  string(15) "JSON_ERROR_NONE"
}
string(5) "Foo\A"
string(3) "Foo"
string(1) "A"
int(1)
bool(false)
NULL
bool(false)
string(%d) "%s"
Constant [ int Foo\A ] { 1 }
object(ReflectionConstant)#2 (1) {
  ["name"]=>
  string(6) "\Foo\A"
}
string(4) "TRUE"
string(0) ""
string(4) "TRUE"
bool(true)
bool(false)
string(4) "Core"
string(4) "Core"
bool(false)
Constant [ <persistent> bool TRUE ] { 1 }
object(ReflectionConstant)#1 (1) {
  ["name"]=>
  string(4) "true"
}
ReflectionException: Constant "NOPE" does not exist
object(ReflectionConstant)#2 (1) {
  ["name"]=>
  string(8) "E_STRICT"
}
Constant [ <persistent, deprecated> int E_STRICT ] { 2048 }
bool(true)
