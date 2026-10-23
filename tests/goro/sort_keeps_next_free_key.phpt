--TEST--
Key-preserving sorts keep the next append index
--FILE--
<?php
foreach (['asort', 'arsort', 'ksort', 'krsort', 'natsort', 'natcasesort', 'sort', 'rsort'] as $f) {
    $a = [3 => 'b', 1 => 'a', 9 => 'z']; unset($a[9]); $f($a); $a[] = 'n'; echo $f, ' ', json_encode(array_keys($a)), "\n";
}
foreach (['uasort', 'uksort', 'usort'] as $f) {
    $a = [3 => 'b', 1 => 'a', 9 => 'z']; unset($a[9]); $f($a, fn($x, $y) => $x <=> $y); $a[] = 'n'; echo $f, ' ', json_encode(array_keys($a)), "\n";
}
$a = [3 => 'b', 1 => 'a', 9 => 'z']; unset($a[9]); array_multisort($a); $a[] = 'n'; echo json_encode(array_keys($a)), "\n";
$a = [5 => 1, 2 => 0]; unset($a[5]); asort($a); $a[] = 9; echo json_encode(array_keys($a)), "\n";
$o = new ArrayObject([3 => 'b', 1 => 'a', 9 => 'z']); unset($o[9]); $o->asort(); $o[] = 'n'; echo json_encode(array_keys($o->getArrayCopy())), "\n";
$o = new ArrayObject([3 => 'b', 1 => 'a', 9 => 'z']); unset($o[9]); $o->uksort(fn($x, $y) => $x <=> $y); $o[] = 'n'; echo json_encode(array_keys($o->getArrayCopy())), "\n";
$a = ['x' => 2, 7 => 1]; arsort($a); $a[] = 0; echo json_encode(array_keys($a)), "\n";
$a = [-5 => 1, -9 => 0]; asort($a); $a[] = 0; echo json_encode(array_keys($a)), "\n";
--EXPECT--
asort [1,3,10]
arsort [3,1,10]
ksort [1,3,10]
krsort [3,1,10]
natsort [1,3,10]
natcasesort [1,3,10]
sort [0,1,2]
rsort [0,1,2]
uasort [1,3,10]
uksort [1,3,10]
usort [0,1,2]
[0,1,2]
[2,6]
[1,3,10]
[1,3,10]
["x",7,8]
[-9,-5,-4]
