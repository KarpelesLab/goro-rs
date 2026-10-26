--TEST--
Closure::__invoke of a closure carries its function's attributes and deprecation
--FILE--
<?php
#[Attribute] class Tag { function __construct(public $v = 0) {} }
class A {
    #[Tag(1), Deprecated] function m($a, $b) {}
}
#[Tag(2)] function f($x) {}
foreach ([Closure::fromCallable([new A, 'm']), f(...), #[Tag(3)] function() {}] as $c) {
    $r = new ReflectionMethod($c, '__invoke');
    echo $r->class, "::", $r->name, " ", $r->getNumberOfParameters(), " ";
    var_dump($r->isDeprecated(), array_map(fn($a) => $a->getName(), $r->getAttributes()));
    foreach ($r->getAttributes('Tag') as $a) var_dump($a->newInstance()->v);
}
--EXPECT--
Closure::__invoke 2 bool(true)
array(2) {
  [0]=>
  string(3) "Tag"
  [1]=>
  string(10) "Deprecated"
}
int(1)
Closure::__invoke 1 bool(false)
array(1) {
  [0]=>
  string(3) "Tag"
}
int(2)
Closure::__invoke 0 bool(false)
array(1) {
  [0]=>
  string(3) "Tag"
}
int(3)
