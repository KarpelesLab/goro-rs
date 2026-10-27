--TEST--
array_walk() over an object with method callbacks, an exception from the callback
--FILE--
<?php
class K { function m(&$v, $k) { $v = "m:$k"; } static function s($v) { echo "s($v)\n"; } }
$o = new stdClass; $o->a = 1; $o->b = 2;
array_walk($o, [new K, "m"]); var_dump($o);
array_walk($o, "K::s");
$re = new TypeError();
array_walk($re, function (&$item, $key) use (&$re) { if ($key === "\x00Error\x00previous") { $item = new Exception(); } });
var_dump(get_class($re->getPrevious()));
array_walk($o, function() { throw new Exception("stop"); });
--EXPECTF--
object(stdClass)#1 (2) {
  ["a"]=>
  string(3) "m:a"
  ["b"]=>
  string(3) "m:b"
}
s(m:a)
s(m:b)
string(9) "Exception"

Fatal error: Uncaught Exception: stop in %s:9
Stack trace:
#0 [internal function]: {closure:%s:9}('m:a', 'a')
#1 %s(9): array_walk(Object(stdClass), Object(Closure))
#2 {main}
  thrown in %s on line 9
