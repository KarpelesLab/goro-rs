--TEST--
Exception::__toString() called as a method keeps its result in the private $string
--FILE--
<?php
$e = new Exception("m", 3, new LogicException("p"));
$s = $e->__toString();
$a = (array)$e;
var_dump($a["\0Exception\0string"] === $s);
var_dump(((array)$e->getPrevious())["\0Exception\0string"]);
$f = new Error("x");
$m = '__toString';
$s = $f->$m();
var_dump(((array)$f)["\0Error\0string"] === $s);
$g = new Exception("y");
$s = call_user_func([$g, '__toString']);
var_dump(((array)$g)["\0Exception\0string"] === $s);
$h = new Exception("z");
$s = (string)$h;
var_dump(((array)$h)["\0Exception\0string"] === $s);
--EXPECTF--
bool(true)
string(0) ""
bool(true)
bool(true)
bool(true)
