<?php
$how_much = 1000000;

$data0 = str_repeat("a", $how_much);
$data1 = str_repeat("b", $how_much);
$i0 = $i1 = 0;
$step = 1024;

while ($i0 < strlen($data0) && $i1 < strlen($data1)) {
    fwrite(STDOUT, substr($data0, $i0, $step));
    fwrite(STDERR, substr($data1, $i1, $step));
    $i0 += $step;
    $i1 += $step;
}

exit(0);
