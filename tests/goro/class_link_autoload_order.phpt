--TEST--
Class linking autoloads the parent, then the traits, then the interfaces
--FILE--
<?php
spl_autoload_register(function ($c) {
    echo "load $c\n";
    switch ($c) {
        case 'A': eval('class A { public function m() {} }'); break;
        case 'T': eval('trait T { public $p = 1; }'); break;
        case 'U': eval('trait U { public function u() {} }'); break;
        case 'I': eval('interface I {}'); break;
        case 'J': eval('interface J {}'); break;
    }
});
class B extends A implements I, J { use T, U; }
echo "ok\n";
var_dump(new B);
--EXPECT--
load A
load T
load U
load I
load J
ok
object(B)#2 (1) {
  ["p"]=>
  int(1)
}
