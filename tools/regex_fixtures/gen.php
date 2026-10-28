<?php
// Fixture generator: php gen.php cases.php extracted.txt errs-list... > fixture
// Each output line: hex(pattern) TAB hex(subject) TAB expected
// expected: ERR <message> | FALSE <preg_last_error> | N <count> then per match
// " | g=start,end ..." (g=- for unset) and " MARK=<hex>".
$cases = require $argv[1];
foreach (file($argv[2], FILE_IGNORE_NEW_LINES) as $l) {
    [$p, $s] = explode("\t", $l);
    $cases[] = [hex2bin($p), hex2bin($s)];
}
if (isset($argv[3])) {
    foreach (file($argv[3], FILE_IGNORE_NEW_LINES) as $l) {
        $parts = explode("\t", $l);
        $cases[] = [hex2bin($parts[1]), ''];
    }
}
foreach ($cases as [$p, $s]) {
    error_clear_last();
    $r = @preg_match_all($p, $s, $m, PREG_SET_ORDER | PREG_OFFSET_CAPTURE);
    $err = error_get_last();
    if ($r === false || $r === null) {
        if ($err) {
            $msg = preg_replace('/^preg_match_all\(\): /', '', $err['message']);
            $exp = "ERR $msg";
        } else {
            $exp = "FALSE " . preg_last_error();
        }
    } else {
        $exp = "N $r";
        foreach ($m as $set) {
            $exp .= " |";
            foreach ($set as $k => $v) {
                if ($k === 'MARK') { $exp .= " MARK=" . bin2hex($v); continue; }
                if (!is_int($k)) continue;
                if ($v[1] < 0) $exp .= " $k=-";
                else $exp .= " $k=" . $v[1] . "," . ($v[1] + strlen($v[0]));
            }
        }
    }
    echo bin2hex($p), "\t", bin2hex($s), "\t", $exp, "\n";
}
