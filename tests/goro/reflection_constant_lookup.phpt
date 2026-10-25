--TEST--
ReflectionConstant: namespaced, special and internal namespaced constants
--FILE--
<?php
namespace Foo {
const A = 1;
}
namespace {
define("N\\C", 3);
const A = 5;
$r = new ReflectionConstant("N\\C"); var_dump($r->name, $r->getValue());
$r = new ReflectionConstant("n\\C"); var_dump($r->name, $r->getValue());
$r = new ReflectionConstant("Foo\\A"); var_dump($r->name, $r->getValue());
echo $r;
$r = new ReflectionConstant('\A'); var_dump($r->name, $r->getValue());
$r = new ReflectionConstant('\Foo\A'); var_dump($r->name, $r->getValue(), $r->getNamespaceName(), $r->getShortName());
foreach (['true', 'TRUE', 'null', 'False'] as $n) {
  $r = new ReflectionConstant($n); var_dump($r->name, $r->getValue(), $r->getExtensionName()); echo $r;
}
$r = new ReflectionConstant('Dom\INDEX_SIZE_ERR'); var_dump($r->name, $r->getValue(), $r->getExtensionName()); echo $r;
$r = new ReflectionConstant('dom\INDEX_SIZE_ERR'); var_dump($r->name);
try { new ReflectionConstant('Dom\NOPE'); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
var_dump(Dom\INDEX_SIZE_ERR);
}
--EXPECT--
string(3) "N\C"
int(3)
string(3) "n\C"
int(3)
string(5) "Foo\A"
int(1)
Constant [ int Foo\A ] { 1 }
string(2) "\A"
int(5)
string(6) "\Foo\A"
int(1)
string(3) "Foo"
string(1) "A"
string(4) "true"
bool(true)
string(4) "Core"
Constant [ <persistent> bool TRUE ] { 1 }
string(4) "TRUE"
bool(true)
string(4) "Core"
Constant [ <persistent> bool TRUE ] { 1 }
string(4) "null"
NULL
string(4) "Core"
Constant [ <persistent> null NULL ] {  }
string(5) "False"
bool(false)
string(4) "Core"
Constant [ <persistent> bool FALSE ] {  }
string(18) "Dom\INDEX_SIZE_ERR"
int(1)
string(3) "dom"
Constant [ <persistent> int Dom\INDEX_SIZE_ERR ] { 1 }
string(18) "dom\INDEX_SIZE_ERR"
Constant "Dom\NOPE" does not exist
int(1)
