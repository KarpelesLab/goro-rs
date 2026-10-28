<?php
// Observation driver for iconv_mime_encode() on the reference PHP 8.5:
//   php85 iconv_obs_mime_encode.php > mime_encode.txt
// One line per case: field name, value, scheme, output charset, line
// length, line break (base64 where binary), then "S:<result base64>" or
// "F:<level>:<message>".
$GLOBALS['errs'] = [];
set_error_handler(function ($no, $msg) {
    $GLOBALS['errs'][] = [$no, preg_replace('/^iconv_mime_encode\(\): /', '', $msg)];
    return true;
});
$values = [
    "\xc3\xa9", str_repeat("\xc3\xa9", 2), str_repeat("\xc3\xa9", 7), str_repeat("\xc3\xa9", 30),
    'abc', str_repeat('abcdefghij', 9), 'a b c', str_repeat('a b ', 20),
    str_repeat("\xe3\x81\x82", 15), "x\xe2\x82\xacy", '', str_repeat("a\xf0\x9f\x98\x80", 6),
];
$names = ['', 'S', 'Subject', str_repeat('N', 30)];
$out = fopen('php://stdout', 'w');
foreach (['B', 'Q'] as $scheme) {
    foreach (['UTF-8', 'ISO-2022-JP', 'UTF-16', 'UTF-7'] as $cs) {
        foreach (["\r\n", "\n"] as $lf) {
            foreach ($names as $name) {
                foreach ($values as $v) {
                    for ($ll = 0; $ll <= 90; $ll += ($ll < 40 ? 1 : 7)) {
                        if ($cs !== 'UTF-8' && ($name !== 'Subject' || $lf !== "\n")) continue;
                        $opts = ['scheme' => $scheme, 'input-charset' => 'UTF-8', 'output-charset' => $cs,
                            'line-length' => $ll, 'line-break-chars' => $lf];
                        $GLOBALS['errs'] = [];
                        $r = iconv_mime_encode($name, $v, $opts);
                        $res = $r === false
                            ? 'F:' . $GLOBALS['errs'][0][0] . ':' . strtr($GLOBALS['errs'][0][1], ' ', '_')
                            : 'S:' . base64_encode($r);
                        fwrite($out, implode(' ', [base64_encode($name), base64_encode($v), $scheme, $cs, $ll, base64_encode($lf), $res]) . "\n");
                    }
                }
            }
        }
    }
}
