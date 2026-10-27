--TEST--
An undefined variable naming a property warns once, at the fetch (reads, compound assignments)
--FILE--
<?php
$o = new stdClass;
$o->{$a}++;
echo "-\n";
$o->{$b} .= $v;
echo "-\n";
$o->{$c} ??= 1;
echo "-\n";
echo $o->{$d};
echo "-\n";
--$o->{$e};
echo "-\n";
var_dump($o);
--EXPECTF--

Warning: Undefined variable $a in %s on line 3

Warning: Undefined property: stdClass::$ in %s on line 3
-

Warning: Undefined variable $b in %s on line 5

Warning: Undefined variable $v in %s on line 5
-

Warning: Undefined variable $c in %s on line 7
-

Warning: Undefined variable $d in %s on line 9
1-

Warning: Undefined variable $e in %s on line 11
-
object(stdClass)#1 (1) {
  [""]=>
  int(0)
}
