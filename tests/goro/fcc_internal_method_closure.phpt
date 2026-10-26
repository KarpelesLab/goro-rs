--TEST--
First-class callables of internal methods are closures over the method
--FILE--
<?php
// First-class callables / Closure::fromCallable() of internal methods are
// closures over the method itself (zend_closure_from_frame()).
$ao = new ArrayObject([1, 2, 'k' => 'v']);
$m = $ao->offsetGet(...);
var_dump($m);
$x = Closure::fromCallable([$ao, "offsetGet"]);
var_dump($x);
foreach ([$m, $x] as $c) {
    var_dump($c(1), $c(key: 'k'), $c->__invoke(0), call_user_func($c, 'k'), call_user_func_array($c, ['key' => 1]));
    var_dump(array_map($c, [0, 1]));
    try { $c(__fcc_captured: 1); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { $c(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { serialize($c); } catch (Exception $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    var_dump($c == $ao->offsetGet(...), $c === $c, $c == $ao->count(...), $c == (new ArrayObject)->offsetGet(...));
    var_dump($c->bindTo(new ArrayObject([5, 6]))(0), $c->call(new ArrayObject([7]), 0));
    var_dump($c->bindTo(new stdClass));
}
var_dump(spl_object_id($m) !== spl_object_id($ao->offsetGet(...)));
$d = (new DateTime("2020-01-01"))->format(...);
var_dump($d("Y"), $d);
$s = DateTime::createFromFormat(...);
var_dump($s('Y-m-d', '2020-01-02')->format('Y'), $s(format: 'Y', datetime: '2021')->format('Y'), $s);
var_dump($s == Closure::fromCallable('DateTime::createFromFormat'), Closure::fromCallable(['DateTime', 'createFromFormat']));
class MyAO extends ArrayObject { function mine() {} }
$my = new MyAO([1, 2]);
var_dump($my->count(...)(), $my->count(...), Closure::fromCallable([$my, 'count'])(), call_user_func([$my, 'count']));
$a = [$my, 'count'];
var_dump($a());
$st = new SplStack;
$push = $st->push(...);
$push(1);
$push(value: 2);
var_dump(count($st));
try { $push(nope: 1); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$h = new SplMinHeap;
try { $h->compare(...); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $ao->nope(...); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECTF--
object(Closure)#2 (3) {
  ["function"]=>
  string(22) "ArrayObject::offsetGet"
  ["this"]=>
  object(ArrayObject)#1 (1) {
    ["storage":"ArrayObject":private]=>
    array(3) {
      [0]=>
      int(1)
      [1]=>
      int(2)
      ["k"]=>
      string(1) "v"
    }
  }
  ["parameter"]=>
  array(1) {
    ["$key"]=>
    string(10) "<required>"
  }
}
object(Closure)#3 (3) {
  ["function"]=>
  string(22) "ArrayObject::offsetGet"
  ["this"]=>
  object(ArrayObject)#1 (1) {
    ["storage":"ArrayObject":private]=>
    array(3) {
      [0]=>
      int(1)
      [1]=>
      int(2)
      ["k"]=>
      string(1) "v"
    }
  }
  ["parameter"]=>
  array(1) {
    ["$key"]=>
    string(10) "<required>"
  }
}
int(2)
string(1) "v"
int(1)
string(1) "v"
int(2)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
Error: Unknown named parameter $__fcc_captured
ArgumentCountError: ArrayObject::offsetGet() expects exactly 1 argument, 0 given
Exception: Serialization of 'Closure' is not allowed
bool(true)
bool(true)
bool(false)
bool(false)
int(5)
int(7)

Warning: Cannot bind method ArrayObject::offsetGet() to object of class stdClass, this will be an error in PHP 9 in %s on line 17
NULL
int(2)
string(1) "v"
int(1)
string(1) "v"
int(2)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
Error: Unknown named parameter $__fcc_captured
ArgumentCountError: ArrayObject::offsetGet() expects exactly 1 argument, 0 given
Exception: Serialization of 'Closure' is not allowed
bool(true)
bool(true)
bool(false)
bool(false)
int(5)
int(7)

Warning: Cannot bind method ArrayObject::offsetGet() to object of class stdClass, this will be an error in PHP 9 in %s on line 17
NULL
bool(true)
string(4) "2020"
object(Closure)#6 (3) {
  ["function"]=>
  string(16) "DateTime::format"
  ["this"]=>
  object(DateTime)#4 (3) {
    ["date"]=>
    string(26) "2020-01-01 00:00:00.000000"
    ["timezone_type"]=>
    int(3)
    ["timezone"]=>
    string(3) "UTC"
  }
  ["parameter"]=>
  array(1) {
    ["$format"]=>
    string(10) "<required>"
  }
}
string(4) "2020"
string(4) "2021"
object(Closure)#7 (2) {
  ["function"]=>
  string(26) "DateTime::createFromFormat"
  ["parameter"]=>
  array(3) {
    ["$format"]=>
    string(10) "<required>"
    ["$datetime"]=>
    string(10) "<required>"
    ["$timezone"]=>
    string(10) "<optional>"
  }
}
bool(true)
object(Closure)#8 (2) {
  ["function"]=>
  string(26) "DateTime::createFromFormat"
  ["parameter"]=>
  array(3) {
    ["$format"]=>
    string(10) "<required>"
    ["$datetime"]=>
    string(10) "<required>"
    ["$timezone"]=>
    string(10) "<optional>"
  }
}
int(2)
object(Closure)#9 (2) {
  ["function"]=>
  string(18) "ArrayObject::count"
  ["this"]=>
  object(MyAO)#8 (1) {
    ["storage":"ArrayObject":private]=>
    array(2) {
      [0]=>
      int(1)
      [1]=>
      int(2)
    }
  }
}
int(2)
int(2)
int(2)
int(2)
Error: Unknown named parameter $nope
Error: Call to protected method SplMinHeap::compare() from global scope
Error: Call to undefined method ArrayObject::nope()
