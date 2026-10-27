--TEST--
get_declared_classes() & co after declarations and aliases
--FILE--
<?php
// get_declared_classes() & co: internal classes first, then the declared
// ones in declaration order; aliases under their lowercased alias name.

$c0 = get_declared_classes();
$i0 = get_declared_interfaces();
$t0 = get_declared_traits();
eval('class Late_A {} interface LateI {} trait LateT {} enum LateE { case X; } class Late__B extends Late_A implements LateI { use LateT; }');
class_alias('Late_A', 'Some\\AliasOfA');
$c1 = array_values(array_diff(get_declared_classes(), $c0));
var_dump($c1);
var_dump(array_values(array_diff(get_declared_interfaces(), $i0)));
var_dump(array_values(array_diff(get_declared_traits(), $t0)));
$all = get_declared_classes();
var_dump(count($all) === count(array_unique($all)));
var_dump(array_search('stdClass', $all) < array_search('Late_A', $all));
var_dump(in_array('Exception', $all), in_array('ArrayAccess', $all), in_array('Traversable', get_declared_interfaces()));
var_dump(in_array('UnitEnum', get_declared_classes()), in_array('UnitEnum', get_declared_interfaces()));
// the internal part does not change between calls
var_dump(array_slice($all, 0, array_search('Late_A', $all)) === array_slice($c0, 0, array_search('Late_A', $all)));
--EXPECT--
array(4) {
  [0]=>
  string(6) "Late_A"
  [1]=>
  string(5) "LateE"
  [2]=>
  string(7) "Late__B"
  [3]=>
  string(13) "some\aliasofa"
}
array(1) {
  [0]=>
  string(5) "LateI"
}
array(1) {
  [0]=>
  string(5) "LateT"
}
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
bool(false)
bool(true)
bool(true)
