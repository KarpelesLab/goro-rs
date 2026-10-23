--TEST--
Arrow functions capture the variables of isset(), empty(), dynamic names, new, instanceof, strings
--FILE--
<?php
$caps = ['read' => true];
$o = new stdClass;
$o->p = 'prop';
$name = 'p';
$cls = 'ArrayObject';
var_dump(array_all(['read'], fn ($cap, $key) => !empty($caps[$cap])));
var_dump((fn ($k) => isset($caps[$k]))('read'));
var_dump((fn () => $o->$name)());
var_dump((fn () => new $cls([1]))()->count());
var_dump((fn () => "x{$caps['read']}y")());
var_dump((fn () => $o instanceof $cls)());
var_dump((fn () => [$ref = &$caps, 1][1])());
?>
--EXPECT--
bool(true)
bool(true)
string(4) "prop"
int(1)
string(3) "x1y"
bool(false)
int(1)
