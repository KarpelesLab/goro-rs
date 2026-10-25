--TEST--
Trait methods are bound before inherited ones (method table order)
--FILE--
<?php
trait T { public $t = 't'; public function tm() {} }
class A { public $a = 'a'; public function am() {} }
class B extends A { use T; public $b = 'b'; public function bm() {} }
var_dump(new B);
print_r(get_class_methods('B'));
print_r(array_map(fn($m) => $m->name, (new ReflectionClass('B'))->getMethods()));
print_r(array_map(fn($p) => $p->name, (new ReflectionClass('B'))->getProperties()));
print_r(get_class_vars('B'));
trait T2 { public function am() { return 'T2'; } }
class C extends A { use T2; public function cm() {} }
print_r(get_class_methods('C'));
--EXPECT--
object(B)#1 (3) {
  ["a"]=>
  string(1) "a"
  ["b"]=>
  string(1) "b"
  ["t"]=>
  string(1) "t"
}
Array
(
    [0] => bm
    [1] => tm
    [2] => am
)
Array
(
    [0] => bm
    [1] => tm
    [2] => am
)
Array
(
    [0] => b
    [1] => t
    [2] => a
)
Array
(
    [b] => b
    [t] => t
    [a] => a
)
Array
(
    [0] => cm
    [1] => am
)
