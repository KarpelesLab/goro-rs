<?php
// Driver for sort_trace.txt: the comparator calls usort()
// makes on permutations of distinct ints, which pin down the comparison
// sequence of PHP's sort. Each line: input : x,y x,y ... => output.
function perms($a) {
    if (count($a) <= 1) return [$a];
    $out = [];
    foreach ($a as $i => $v) {
        $rest = $a; unset($rest[$i]);
        foreach (perms(array_values($rest)) as $p) $out[] = array_merge([$v], $p);
    }
    return $out;
}
$inputs = [];
for ($n = 0; $n <= 6; $n++) foreach (perms(range(1, $n)) as $p) $inputs[] = $p;
mt_srand(42);
for ($n = 7; $n <= 16; $n++) for ($k = 0; $k < 12; $k++) { $a = range(1, $n); shuffle($a); $inputs[] = $a; }
for ($n = 17; $n <= 40; $n++) { $inputs[] = range(1, $n); $inputs[] = array_reverse(range(1, $n)); for ($k = 0; $k < 6; $k++) { $a = range(1, $n); shuffle($a); $inputs[] = $a; } }
foreach ([50, 64, 100, 129, 200, 500, 1000, 1025, 2000, 5000] as $n) { $inputs[] = range(1, $n); $inputs[] = array_reverse(range(1, $n)); for ($k = 0; $k < 3; $k++) { $a = range(1, $n); shuffle($a); $inputs[] = $a; } }
foreach ($inputs as $in) {
    $log = [];
    $a = $in;
    usort($a, function ($x, $y) use (&$log) { $log[] = "$x,$y"; return $x <=> $y; });
    echo implode(' ', $in), ' : ', implode(' ', $log), ' => ', implode(' ', $a), "\n";
}
// Driver for sort_trace2.txt (one php85 run): comparator calls of usort()
// around the median-of-five threshold and for odd sizes.
mt_srand(7);
$inputs = [];
foreach ([1001, 1010, 1020, 1023, 1024, 1025, 1027, 1029, 1031, 3001, 4097] as $n) { $a = range(1, $n); shuffle($a); $inputs[] = $a; $inputs[] = range(1, $n); }
foreach ($inputs as $in) {
    $log = [];
    $a = $in;
    usort($a, function ($x, $y) use (&$log) { $log[] = "$x,$y"; return $x <=> $y; });
    echo implode(' ', $in), ' : ', implode(' ', $log), ' => ', implode(' ', $a), "\n";
}
