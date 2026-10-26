--TEST--
Closure bound to an object without a scope: self is Closure; constant redeclaration names the lowercased namespace; @strict-properties internal classes refuse dynamic properties
--FILE--
<?php
namespace Foo;
class O { public $p = 1; private function pm() {} }
$f = function () { return [self::class, static::class, get_class($this)]; };
$g = \Closure::bind($f, new O, null);
var_dump($g());
class P { private $z = 1; private function m() {} }
try { $h = \Closure::bind(function () { return $this->m(); }, new P, null); var_dump($h()); } catch (\Error $e) { echo $e->getMessage(), "\n"; }
const A = 1;
const A = 2;
\define('Foo\B', 1);
\define('Foo\B', 2);
\define('FOO\C', 1);
\define('foo\C', 2);
try { dir('/')->dyn = 1; } catch (\Error $e) { echo $e->getMessage(), "\n"; }
$d = dir('/');
try { $d->path = 'x'; } catch (\Error $e) { echo $e->getMessage(), "\n"; }
try { $w = new \WeakMap; $w->x = 1; } catch (\Error $e) { echo $e->getMessage(), "\n"; }
try { $r = new \Random\Randomizer; $r->x = 1; } catch (\Error $e) { echo $e->getMessage(), "\n"; }
class MyErr extends \DateError {}
$e = new MyErr;
$e->dyn = 1;
var_dump($e->dyn);
try { $e = new \DateError; $e->dyn = 1; } catch (\Error $x) { echo $x->getMessage(), "\n"; }
--EXPECTF--
array(3) {
  [0]=>
  string(7) "Closure"
  [1]=>
  string(5) "Foo\O"
  [2]=>
  string(5) "Foo\O"
}
Call to private method Foo\P::m() from scope Closure

Warning: Constant foo\A already defined, this will be an error in PHP 9 in %s on line 10

Warning: Constant foo\B already defined, this will be an error in PHP 9 in %s on line 12

Warning: Constant foo\C already defined, this will be an error in PHP 9 in %s on line 14
Cannot create dynamic property Directory::$dyn
Cannot modify readonly property Directory::$path
Cannot create dynamic property WeakMap::$x
Cannot create dynamic property Random\Randomizer::$x

Deprecated: Creation of dynamic property Foo\MyErr::$dyn is deprecated in %s on line 22
int(1)
Cannot create dynamic property DateError::$dyn
