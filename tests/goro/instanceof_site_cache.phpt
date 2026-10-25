--TEST--
instanceof of objects whose class the site saw before, aliases and classes declared later
--FILE--
<?php
interface I {} interface J extends I {}
class A implements J {} class B extends A {} class C {}
enum E: int implements I { case X = 1; }
function chk($o) { return [$o instanceof I, $o instanceof J, $o instanceof A, $o instanceof Later, $o instanceof Alias, $o instanceof Traversable, $o instanceof UnitEnum, $o instanceof BackedEnum, $o instanceof Closure]; }
class P { function t($o) { return [$o instanceof self, $o instanceof static]; } }
class Q extends P {}
$objs = [new A, new B, new C, E::X, function () {}, new ArrayIterator([])];
for ($i = 0; $i < 3; $i++) {
    foreach ($objs as $o) echo json_encode(chk($o)), " ";
    echo "\n";
    if ($i == 0) { class_alias('C', 'Alias'); eval('class Later extends B {}'); $objs[] = new Later; }
    echo json_encode((new P)->t(new Q)), json_encode((new Q)->t(new P)), json_encode((new Q)->t(new Q)), "\n";
}
--EXPECTF--
[true,true,true,false,false,false,false,false,false] [true,true,true,false,false,false,false,false,false] [false,false,false,false,false,false,false,false,false] [true,false,false,false,false,false,true,true,false] [false,false,false,false,false,false,false,false,true] [false,false,false,false,false,true,false,false,false] 
[true,true][true,false][true,true]
[true,true,true,false,false,false,false,false,false] [true,true,true,false,false,false,false,false,false] [false,false,false,false,true,false,false,false,false] [true,false,false,false,false,false,true,true,false] [false,false,false,false,false,false,false,false,true] [false,false,false,false,false,true,false,false,false] [true,true,true,true,false,false,false,false,false] 
[true,true][true,false][true,true]
[true,true,true,false,false,false,false,false,false] [true,true,true,false,false,false,false,false,false] [false,false,false,false,true,false,false,false,false] [true,false,false,false,false,false,true,true,false] [false,false,false,false,false,false,false,false,true] [false,false,false,false,false,true,false,false,false] [true,true,true,true,false,false,false,false,false] 
[true,true][true,false][true,true]
