<?php
// Fixture driver for encoding.rs: run once on PHP 8.5 (observed behaviour).
//   php85 -d error_reporting=-1 -d display_errors=1 -d log_errors=0 -d html_errors=0 encoding.php > encoding_full.txt
require __DIR__ . '/common.inc';

$all = '';
for ($i = 0; $i < 256; $i++) $all .= chr($i);
label($all, 'bytes:0-256');
$texts = ['', 'a', 'abc', 'The quick brown fox jumped over the lazy dog.', "\0", "\0\0\0", $all,
    str_repeat('x', 1000), "123456789", "Hello World\n", "\xff\xfe\xfd"];

foreach ($texts as $t) {
    run('bin2hex', $t);
    run('crc32', $t);
    run('md5', $t);
    run('md5', $t, true);
    run('sha1', $t);
    run('sha1', $t, true);
    run('str_rot13', $t);
    run('quoted_printable_encode', $t);
    run('convert_uuencode', $t);
}
$halpha = ['0', '9', 'a', 'f', 'A', 'F', 'g', ' ', 'x', "\0"];
for ($i = 0; $i < 300; $i++) run('hex2bin', rnd_str($halpha, 6));
run('hex2bin', bin2hex($all));
run('hex2bin', 'AH');
run('hex2bin', 'HA');

// quoted-printable
$qalpha = ['a', 'a', 'a', ' ', "\t", "\r", "\n", "\r\n", '=', "\0", "\xff", "\x7f", "\x80", '.', '-', 'é', '~'];
for ($i = 0; $i < 1500; $i++) {
    $s = rnd_str($qalpha, 1 + rnd(3) * 60);
    run('quoted_printable_encode', $s);
}
run('quoted_printable_encode', str_repeat("\0", 200));
run('quoted_printable_encode', str_repeat("строка в юникоде", 50));
run('quoted_printable_encode', str_repeat("a", 200));
run('quoted_printable_encode', str_repeat("a", 75) . "\r\n" . str_repeat("b", 76) . "\n" . str_repeat("c", 74) . ' ');
$dalpha = ['=', '=', 'A', 'f', '0', '9', 'g', ' ', "\t", "\r", "\n", 'x', "=\r\n", "=\n", '=3D', '=4', "\xff"];
for ($i = 0; $i < 1500; $i++) run('quoted_printable_decode', rnd_str($dalpha, 8));
run('quoted_printable_decode', "=FAwow-factor=C1=d0=D5=DD=C5=CE=CE=D9=C5=0A=\n=20=D4=cf=D2=C7=CF=D7=D9=C5=\n=20=\n=D0=\n=D2=CF=C5=CB=D4=D9");

// uuencode
for ($i = 0; $i < 200; $i++) {
    $n = rnd(100);
    $s = '';
    for ($k = 0; $k < $n; $k++) $s .= chr(rnd(256));
    run('convert_uuencode', $s);
    $e = convert_uuencode($s);
    run('convert_uudecode', $e);
    run('convert_uudecode', substr($e, 0, rnd(strlen($e) + 1)));
}
$uualpha = ['!', '"', '#', 'M', '`', ' ', "\n", 'a', '~', "\x7f", '0', '=', "\r"];
for ($i = 0; $i < 600; $i++) run('convert_uudecode', rnd_str($uualpha, 12));
run('convert_uuencode', "~!@#$%^&*()_}{POIUYTREWQQSDFGHJKL:<MNBVCXZ");
run('convert_uudecode', "!@#$%^YUGFDFGHJKLUYTFBNMLOYT");
run('convert_uudecode', "");
run('convert_uudecode', convert_uuencode("not very sophisticated"));
run('convert_uudecode', substr(convert_uuencode("not very sophisticated"), 0, -10));
echo "# done\n";
