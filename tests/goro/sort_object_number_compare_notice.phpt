--TEST--
Sorting an object against a number notices the failed cast
--FILE--
<?php
$o = new ArrayObject([5]); $a = [1, $o]; sort($a); echo count($a), "\n";
$s = new stdClass; $a = [2.5, $s, 3]; rsort($a); echo count($a), "\n";
$a = ['k' => $s, 'j' => 1]; asort($a); echo implode(",", array_keys($a)), "\n";
$a = [$s, "x"]; sort($a);
var_dump(max(1, $s) === $s, min([$s, 2]));
var_dump(in_array(1, [$s]), array_search(1, [$s]));
$a = [$s, 1]; var_dump(array_unique($a, SORT_REGULAR) == $a);
--EXPECTF--

Notice: Object of class ArrayObject could not be converted to int in %s on line 2
2

Notice: Object of class stdClass could not be converted to float in %s on line 3

Notice: Object of class stdClass could not be converted to int in %s on line 3
3

Notice: Object of class stdClass could not be converted to int in %s on line 4
k,j

Notice: Object of class stdClass could not be converted to int in %s on line 6

Notice: Object of class stdClass could not be converted to int in %s on line 6
bool(false)
object(stdClass)#2 (0) {
}

Notice: Object of class stdClass could not be converted to int in %s on line 7

Notice: Object of class stdClass could not be converted to int in %s on line 7
bool(true)
int(0)

Notice: Object of class stdClass could not be converted to int in %s on line 8

Notice: Object of class stdClass could not be converted to int in %s on line 8
bool(false)
