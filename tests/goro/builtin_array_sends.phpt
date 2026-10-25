--TEST--
Array temporaries sent to builtins' by-value and by-reference parameters
--FILE--
<?php
function mk() { return [3, 1, 2]; }
function run() {
    $a = ['x' => 1, 'y' => 2];
    for ($i = 0; $i < 2; $i++) {
        echo count(array_keys($a)), implode(',', array_merge(mk(), [$i])), json_encode(array_flip(mk())), "\n";
        echo implode(',', array_map(fn($v) => $v * 2, mk())), ' ', max(mk()), ' ', in_array(2, mk()) ? 'y' : 'n', "\n";
        echo end(mk()), ' ', json_encode(array_slice(mk(), 1)), "\n";
        $s = mk();
        sort($s);
        echo implode(',', $s), ' ', array_sum(array_values($a + ['z' => 3])), "\n";
    }
}
run();
?>
--EXPECTF--
23,1,2,0{"3":0,"1":1,"2":2}
6,2,4 3 y

Notice: Only variables should be passed by reference in %s on line 8
2 [1,2]
1,2,3 6
23,1,2,1{"3":0,"1":1,"2":2}
6,2,4 3 y

Notice: Only variables should be passed by reference in %s on line 8
2 [1,2]
1,2,3 6
