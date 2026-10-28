<?php
// Observation driver: checksums of the full behaviour of every accepted
// charset name on the reference PHP 8.5 (glibc 2.43 iconv), to split alias
// groups that iconv_obs_names.php's short fingerprint merged.
//   php85 iconv_obs_aliases.php names.out > aliases.out
// Output: "A <name> <dec-crc> <enc-crc>" (same sequences as iconv_fixture.php).
$GLOBALS['lasterr'] = '';
set_error_handler(function ($no, $msg) { $GLOBALS['lasterr'] = $msg; return true; });
function k($f, $t, $s) {
    $GLOBALS['lasterr'] = '';
    $r = iconv($f, $t, $s);
    if ($r !== false) return $r === '' ? 'e' : bin2hex($r);
    $e = $GLOBALS['lasterr'];
    return strpos($e, 'incomplete') !== false ? 'i' : (strpos($e, 'illegal') !== false ? 'x' : 'W');
}
function block($lo, $hi) {
    $s = '';
    for ($cp = $lo; $cp < $hi; $cp++) {
        if ($cp >= 0xD800 && $cp < 0xE000) continue;
        $s .= pack('N', $cp);
    }
    return $s;
}
$blocks = [];
for ($b = 0; $b < 0x10000; $b += 0x1000) $blocks[] = block($b, $b + 0x1000);
foreach (file($argv[1], FILE_IGNORE_NEW_LINES) as $line) {
    $p = explode("\t", $line);
    if ($p[0] !== 'ACCEPT') continue;
    $cs = $p[1];
    if (strpos($cs, '//') !== false || trim($cs) !== $cs || strpos($cs, ' ') !== false) continue;
    $dec = [];
    $queue = [''];
    for ($q = 0; $q < count($queue); $q++) {
        $pre = $queue[$q];
        for ($b = 0; $b < 256; $b++) {
            $s = $pre . chr($b);
            $r = k($cs, 'UTF-32BE', $s);
            $dec[] = $r;
            if ($r === 'i' && strlen($s) < 3) $queue[] = $s;
        }
    }
    $enc = [];
    foreach ($blocks as $blk) $enc[] = k('UTF-32BE', $cs . '//IGNORE', $blk);
    printf("A %s %08x %08x\n", $cs, crc32(implode(',', $dec)), crc32(implode(',', $enc)));
}
