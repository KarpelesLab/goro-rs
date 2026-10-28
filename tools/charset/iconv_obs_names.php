<?php
// Observation driver: which charset names does PHP's iconv accept, and which
// of them behave identically? Run on the reference PHP 8.5 (glibc 2.43 iconv):
//   php85 iconv_obs_names.php names.txt > names.out
// For each accepted name it prints a fingerprint built from decoding every
// single byte and encoding a sample of code points; names sharing a
// fingerprint are aliases of one charset.
$GLOBALS['lasterr'] = '';
set_error_handler(function ($no, $msg) { $GLOBALS['lasterr'] = $msg; return true; });
function conv($from, $to, $s) {
    $GLOBALS['lasterr'] = '';
    $r = iconv($from, $to, $s);
    if ($r !== false) return bin2hex($r);
    $e = $GLOBALS['lasterr'];
    if (strpos($e, 'Wrong encoding') !== false) return 'W';
    if (strpos($e, 'incomplete') !== false) return 'i';
    if (strpos($e, 'illegal') !== false) return 'x';
    return '?' . $e;
}
$sample = [0x41, 0x0A, 0x7E, 0x5C, 0xA0, 0xA5, 0xA6, 0xE9, 0xFC, 0x0100, 0x0152,
    0x0401, 0x0410, 0x03B1, 0x05D0, 0x0627, 0x0E01, 0x1E00, 0x2014, 0x2016, 0x2022,
    0x203E, 0x20AC, 0x2116, 0x2212, 0x2225, 0x2500, 0x3000, 0x3042, 0x301C, 0x4E00,
    0x4E02, 0x4E2A, 0x5341, 0x9AD9, 0xAC00, 0xAC02, 0xD7A3, 0xE000, 0xE5E5, 0xFF0D,
    0xFF5E, 0xFF61, 0xFFE5, 0xFEFF, 0x10000, 0x1F600, 0x20000, 0x0080, 0x00FF, 0x0131];
$names = file($argv[1], FILE_IGNORE_NEW_LINES);
foreach ($names as $name) {
    $in = conv($name, 'UTF-8', '');
    $out = conv('UTF-8', $name, '');
    if ($in === 'W' && $out === 'W') { echo "REJECT\t$name\n"; continue; }
    $dec = [];
    for ($b = 0; $b < 256; $b++) $dec[] = conv($name, 'UTF-32BE', chr($b));
    $enc = [];
    foreach ($sample as $cp) $enc[] = conv('UTF-32BE', $name, pack('N', $cp));
    $enc[] = conv('UTF-8', $name, 'ab');
    $enc[] = conv($name, 'UTF-8', 'ABCD');
    $fp = implode(',', $dec) . '|' . implode(',', $enc);
    echo "ACCEPT\t$name\t", ($in === 'W' ? 'noin' : 'in'), ($out === 'W' ? '-noout' : '-out'), "\t", md5($fp), "\t", $fp, "\n";
}
