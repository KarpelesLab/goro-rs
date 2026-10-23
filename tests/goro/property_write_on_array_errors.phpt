--TEST--
Assigning or incrementing a property of an array throws
--FILE--
<?php
$x=[0]; try{$x->p=1;echo "no error\n";}catch(Error $e){echo $e->getMessage(),"\n";}
$x=[1]; try{$x->p.=1;}catch(Error $e){echo $e->getMessage(),"\n";}
$x=[1]; try{$x->p++;}catch(Error $e){echo $e->getMessage(),"\n";}
$x=[1]; try{--$x->p;}catch(Error $e){echo $e->getMessage(),"\n";}
$x=[1]; try{$x->p[] = 1;}catch(Error $e){echo $e->getMessage(),"\n";}
$x=[1]; try{$x->p->q = 1;}catch(Error $e){echo $e->getMessage(),"\n";}
$x=[1]; try{$r = &$x->p;}catch(Error $e){echo $e->getMessage(),"\n";}
$x=[1]; try{unset($x->p);}catch(Error $e){echo $e->getMessage(),"\n";}
$x=[1]; var_dump(isset($x->p), $x->p ?? 'd');
function f() { $x=[0]; try{$x->p=1;echo "no error\n";}catch(Error $e){echo $e->getMessage(),"\n";} $n = "q"; try{$x->$n=1;}catch(Error $e){echo $e->getMessage(),"\n";} }
f();
var_dump($x);
--EXPECT--
Attempt to assign property "p" on array
Attempt to assign property "p" on array
Attempt to increment/decrement property "p" on array
Attempt to increment/decrement property "p" on array
Attempt to modify property "p" on array
Attempt to modify property "p" on array
Attempt to modify property "p" on array
bool(false)
string(1) "d"
Attempt to assign property "p" on array
Attempt to assign property "q" on array
array(1) {
  [0]=>
  int(1)
}
