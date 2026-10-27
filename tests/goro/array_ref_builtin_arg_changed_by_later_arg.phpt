--TEST--
array_push($a, f()) where f() replaces $a: the builtin works on the array $a holds when it is called
--FILE--
<?php
$o = new stdClass; $o->w = ['x', 'F'];
array_push($o->w, (function () use ($o) { $o->w = ['q']; return 1; })());
echo json_encode($o->w), "\n";
$o->w = ['x', 'F'];
array_push($o->w, (function () use ($o) { $o->w[] = 'q'; return 1; })());
echo json_encode($o->w), "\n";
$o->w = ['x', 'F'];
sort($o->w, (function () use ($o) { $o->w = [3, 2, 1]; return 0; })());
echo json_encode($o->w), "\n";
$a = ['x', 'F'];
array_push($a, (function () use (&$a) { $a = ['q']; return 1; })());
echo json_encode($a), "\n";

?>
--EXPECT--
["q",1]
["x","F","q",1]
[1,2,3]
["q",1]
