--TEST--
An arrow function's end line is the line of the token after its body (CG(zend_lineno) at the reduction)
--FILE--
<?php
$a = fn () => 1

;
$b = [fn () => "a
b
c"
 , 1];
$c = fn () => (1
)
 ;
foreach ([$a, $b[0], $c] as $t) { $r = new ReflectionFunction($t); echo $r->getStartLine(), "-", $r->getEndLine(), "\n"; }

?>
--EXPECT--
2-4
5-8
9-11
