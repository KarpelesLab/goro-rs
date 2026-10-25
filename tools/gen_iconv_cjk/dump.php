<?php
// Black-box dumps of glibc's iconv (run with a php built against glibc):
//   php -n dump.php CS E [max-hex]                      every code point encoded alone
//   php -n dump.php CS D1 [state-hex]                   every byte decoded alone
//   php -n dump.php CS D2 [state-hex] [lead-lo-hex]     every lead >= lo + trail
//   php -n dump.php CS IGN [state-hex] [lead-lo-hex] [prefix-hex] [tail-hex]
//                                                       illegal lead+trail: which
//                                                       skips explain //IGNORE
// state-hex: bytes put before each sequence (escapes, a 0x8E plane prefix...;
// for IGN it is part of the sequence when it is not an escape: pass it as
// the 5th argument).
$cs = $argv[1];
$mode = $argv[2];
$st = hex2bin($argv[3] ?? '');
$lo = hexdec($argv[4] ?? '80');
$pre = hex2bin($argv[5] ?? '');

function dec($cs, $s)
{
    error_clear_last();
    $r = @iconv($cs, "UTF-32BE", $s);
    if ($r === false) {
        $e = error_get_last();
        return str_contains($e['message'] ?? '', 'incomplete') ? "INC" : "ILL";
    }
    return $r === '' ? 'EMPTY' : bin2hex($r);
}

function dig($cs, $s)
{
    $r = @iconv($cs, "UTF-32BE//IGNORE", $s);
    return $r === false ? "F" : bin2hex($r);
}

switch ($mode) {
    case 'E':
        $max = hexdec($argv[3] ?? '10FFFF');
        for ($cp = 0; $cp <= $max; $cp++) {
            if ($cp >= 0xD800 && $cp < 0xE000) {
                continue;
            }
            $r = @iconv("UTF-32BE", $cs, pack('N', $cp));
            if ($r !== false) {
                echo dechex($cp), " ", bin2hex($r), "\n";
            }
        }
        break;
    case 'D1':
        for ($b = 0; $b < 256; $b++) {
            echo bin2hex(chr($b)), " ", dec($cs, $st . chr($b)), "\n";
        }
        break;
    case 'D2':
        for ($b = $lo; $b < 256; $b++) {
            for ($c = 0; $c < 256; $c++) {
                $s = chr($b) . chr($c);
                $r = dec($cs, $st . $s);
                if ($r !== 'ILL') {
                    echo bin2hex($s), " $r\n";
                }
            }
        }
        break;
    case 'IGN':
        // ("auto": a two-byte character first, so that a trail byte taken
        // as a lead byte shows)
        $t = $argv[6] ?? '';
        $tail = ($t === 'auto' ? iconv("UTF-8", $cs, "\u{4E00}") : hex2bin($t)) . "AAAAAA";
        for ($b = $lo; $b < 256; $b++) {
            for ($c = 0; $c < 256; $c++) {
                $s = $pre . chr($b) . chr($c);
                if (@iconv($cs, "UTF-32BE", $st . $s . $tail) !== false) {
                    continue;
                }
                $r = dig($cs, $st . $s . $tail);
                $k = '';
                for ($n = 1; $n <= strlen($s); $n++) {
                    if (dig($cs, $st . substr($s, $n) . $tail) === $r) {
                        $k .= $n;
                    }
                }
                echo bin2hex(chr($b) . chr($c)), " ", $k === '' ? '?' : $k, "\n";
            }
        }
        break;
}
