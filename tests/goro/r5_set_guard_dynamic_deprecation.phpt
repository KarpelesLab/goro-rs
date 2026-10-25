--TEST--
A write inside __set() creating a dynamic property deprecates
--FILE--
<?php
class G {
    public $decl;
    function __set($n, $v) { echo "set $n\n"; $this->$n = $v; }
}
$g = new G;
$g->abc = 1;
unset($g->decl);
$g->decl = 2;
var_dump($g);
#[AllowDynamicProperties]
class H { function __set($n, $v) { $this->$n = $v; } }
$h = new H;
$h->x = 1;
var_dump($h);
--EXPECTF--
set abc

Deprecated: Creation of dynamic property G::$abc is deprecated in %s on line 4
set decl
object(G)#1 (2) {
  ["decl"]=>
  int(2)
  ["abc"]=>
  int(1)
}
object(H)#2 (1) {
  ["x"]=>
  int(1)
}
