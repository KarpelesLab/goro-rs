--TEST--
Dynamic properties of internal objects created by a reference / dim / ++ fetch are deprecated
--FILE--
<?php
$d = (new DOMDocument)->createElement("e"); $d->zz[] = 1; $r3 = &$d->yy;
$ao2 = new ArrayObject([]); $ao2->q[] = 1;
$dt = new DateTime; $dt->foo[] = 1;
try { $c = function(){}; $c->x[] = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
$w = new WeakMap; try { $w->p[] = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
$f = new SplFixedArray(1); $f->p[] = 1; $r4 = &$f->q;
class X {}
$x = new X; $r = &$x->nope; $x->a[] = 1; $x->b++; 
$e = new Exception; $r2 = &$e->nope2; $e->c[] = 1; $e->d++; $e->f = 1;
var_dump(array_keys(get_object_vars($x)));
--EXPECTF--

Deprecated: Creation of dynamic property DOMElement::$zz is deprecated in %s on line 2

Deprecated: Creation of dynamic property DOMElement::$yy is deprecated in %s on line 2

Deprecated: Creation of dynamic property ArrayObject::$q is deprecated in %s on line 3

Deprecated: Creation of dynamic property DateTime::$foo is deprecated in %s on line 4
Cannot create dynamic property Closure::$x
Cannot create dynamic property WeakMap::$p

Deprecated: Creation of dynamic property SplFixedArray::$p is deprecated in %s on line 7

Deprecated: Creation of dynamic property SplFixedArray::$q is deprecated in %s on line 7

Deprecated: Creation of dynamic property X::$nope is deprecated in %s on line 9

Deprecated: Creation of dynamic property X::$a is deprecated in %s on line 9

Deprecated: Creation of dynamic property X::$b is deprecated in %s on line 9

Warning: Undefined property: X::$b in %s on line 9

Deprecated: Creation of dynamic property Exception::$nope2 is deprecated in %s on line 10

Deprecated: Creation of dynamic property Exception::$c is deprecated in %s on line 10

Deprecated: Creation of dynamic property Exception::$d is deprecated in %s on line 10

Warning: Undefined property: Exception::$d in %s on line 10

Deprecated: Creation of dynamic property Exception::$f is deprecated in %s on line 10
array(3) {
  [0]=>
  string(4) "nope"
  [1]=>
  string(1) "a"
  [2]=>
  string(1) "b"
}
