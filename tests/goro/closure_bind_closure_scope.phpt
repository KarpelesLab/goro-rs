--TEST--
Closure::bind()/bindTo() to the internal Closure scope warns unless the closure already has it
--FILE--
<?php
$f = function() { return isset($this) ? get_class($this) : 'none'; };
var_dump($f->bindTo(new stdClass, 'Closure'));
$g = $f->bindTo(new stdClass);
var_dump($g());
var_dump($g->bindTo(new stdClass, 'Closure') !== null);
var_dump($g->bindTo(new stdClass, $g) !== null);
var_dump($f->bindTo(new stdClass, $g));
var_dump(Closure::bind($f, null, 'Closure'));
?>
--EXPECTF--

Warning: Cannot bind closure to scope of internal class Closure, this will be an error in PHP 9 in %s on line 3
NULL
string(8) "stdClass"
bool(true)
bool(true)

Warning: Cannot bind closure to scope of internal class Closure, this will be an error in PHP 9 in %s on line 8
NULL

Warning: Cannot bind closure to scope of internal class Closure, this will be an error in PHP 9 in %s on line 9
NULL
