<?php
// Observation driver for iconv_mime_decode() on the reference PHP 8.5:
//   php85 iconv_obs_mime.php > mime_decode.txt
// Prints one line per input: the input in base64, then for each mode 0..3
// " S:<result in base64>" or " F:<diagnostic level>:<message>" (the first
// diagnostic). The inputs are every sequence of up to three tokens
// from a small alphabet of header fragments, plus a few longer cases.
$GLOBALS['errs'] = [];
set_error_handler(function ($no, $msg) {
    $GLOBALS['errs'][] = [$no, preg_replace('/^iconv_mime_decode\(\): /', '', $msg)];
    return true;
});
$tokens = ['a', ' ', "\t", "\r\n", "\n", "\r", '=', '?', '=?', '?=', '_', "\x80", "\xc3\xa9", ':', 'Subject: ',
    '=?UTF-8?B?w6k=?=', '=?UTF-8?Q?=C3=A9?=', '=?utf-8?q?a_b?=', '=?bogus?B?YQ==?=', '=?UTF-8?Q?=G?=', '=?UTF-8?B?w6?=',
    '=?UTF-8?X?YQ==?=', '=?UTF-8?B?YQ', '=?UTF-8*en?Q?a?=', '=??Q?a?='];
$inputs = [''];
$n = count($tokens);
for ($i = 0; $i < $n; $i++) {
    $inputs[] = $tokens[$i];
    for ($j = 0; $j < $n; $j++) {
        $inputs[] = $tokens[$i] . $tokens[$j];
        for ($k = 0; $k < $n; $k++) $inputs[] = $tokens[$i] . $tokens[$j] . $tokens[$k];
    }
}
$out = fopen('php://stdout', 'w');
foreach ($inputs as $s) {
    $row = base64_encode($s);
    foreach ([0, 1, 2, 3] as $m) {
        $GLOBALS['errs'] = [];
        $r = iconv_mime_decode($s, $m, 'UTF-8');
        $row .= $r === false
            ? ' F:' . $GLOBALS['errs'][0][0] . ':' . strtr($GLOBALS['errs'][0][1], ' ', '_')
            : ' S:' . base64_encode($r);
    }
    fwrite($out, $row . "\n");
}
