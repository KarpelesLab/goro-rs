--TEST--
strftime() follows glibc's flags, widths and modifiers
--FILE--
<?php
$t = 1704110745; // 2024-01-01 12:05:45 UTC, a Monday
foreach (['%^a', '%^B', '%#Z', '%#a', '%#p', '%^P', '%05d', '%5Y', '%10H', '%-5d', '%_5H', '%010a', '%10z', '%^c', '%20T', '%10s',
          '%^', '%#', '%-', '%E', '%O', '%5', '%E%', '%5E%', '%Ed', '%5Ed', '%Ob', '%EY', '%OY', '%Ex', "%Y\0%m"] as $f) {
    var_dump(@gmstrftime($f, $t));
}
var_dump(@gmstrftime('%C', -42286608000)); // year 630
var_dump(@gmstrftime('%C %Y %5Y %05Y %_6Y %10G', -50000000000000));
var_dump(@gmstrftime('%4095Y', 0) === str_repeat('0', 4091) . '1970', @gmstrftime('%4096Y', 0), @gmstrftime(str_repeat('x', 4096)));
$specs = str_split('aAbBcCdDeFgGhHIjklmMnpPrRsStTuUVwWxXyYzZ%EOq');
$flags = ['', '_', '-', '0', '^', '#', '^#', '5', '05', '_5', '-5', '10', '^10', '#10', 'E', 'O', '5E', '0O', '010', '-010', '_010', '03', '1'];
foreach ([$t, 0, -62000000000, -50000000000000, 253402300799] as $ts) {
    $line = '';
    foreach ($specs as $s) foreach ($flags as $fl) {
        $f = '%' . $fl . $s;
        $line .= "$f=" . @gmstrftime("[$f]", $ts) . "\n";
    }
    echo md5($line), "\n";
}
--EXPECT--
string(3) "MON"
string(7) "JANUARY"
string(3) "gmt"
string(3) "MON"
string(2) "pm"
string(2) "pm"
string(5) "00001"
string(5) "02024"
string(10) "0000000012"
string(5) "    1"
string(5) "   12"
string(10) "0000000Mon"
string(20) "         +0000000000"
string(24) "MON JAN  1 12:05:45 2024"
string(20) "            12:05:45"
string(10) "1704110745"
string(2) "%^"
string(2) "%#"
string(2) "%-"
string(2) "%E"
string(2) "%O"
string(5) "   %5"
string(1) "%"
string(5) "    %"
string(3) "%Ed"
string(5) " %5Ed"
string(3) "Jan"
string(4) "2024"
string(3) "%OY"
string(8) "01/01/24"
string(4) "2024"
string(1) "6"
string(53) "-15825 -1582467 -1582467 -1582467 -1582467 -001582467"
bool(true)
bool(false)
bool(false)
71bab68f37c0f96fcb4411dcc431c824
601ee897d57a5cca640c4eaacc0c4e4e
979ac2d0f1bc6e77faea565691653c21
d6b9c8897ff6d03fa32d798d1830ed88
5fd1846a67cb22acf53c1af4b5d12bf6
