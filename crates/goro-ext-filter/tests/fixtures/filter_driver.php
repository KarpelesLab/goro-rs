<?php
// Fixture driver for goro-ext-filter, run once on PHP 8.5:
//
//   php -n -d error_reporting=-1 -d display_errors=1 -d precision=14 \
//       -d serialize_precision=-1 -d default_charset=UTF-8 \
//       filter_driver.php > filter_php85.txt
//
// It only generates inputs and records what the filter functions return;
// the inputs are written to the fixture, so the Rust test does not need
// to reproduce the generation.
//
// Line formats (fields separated by one space; strings escaped by esc()):
//
//   O <n> <esc(serialize(options))>     declares option set n (int or array)
//   I <esc(input)>                      sets the current input (a string)
//   J <esc(serialize(input))>           sets the current input (other types)
//   V <filter> <n> <result>             filter_var(input, filter, options n)
//   A <filter-or-n> <add_empty> <result>
//                                       filter_var_array(input, options n
//                                       when the first field is "o<n>",
//                                       else the int filter)
//   U <first> <last> <atom><quoted>     code points first..last (hex) in
//                                       "a<c>b@b.cd" and "\"a<c>b\"@b.cd"
//                                       with FILTER_FLAG_EMAIL_UNICODE are
//                                       valid (1) or not (0)
//
// <result> is "=" when the result is identical (===) to the input, "F",
// "T" or "N" for false, true and null, "R<esc(serialize(result))>"
// otherwise, or "X<class>:<esc(message)>" for
// an exception. Warnings and deprecations raised during the call follow as
// extra fields "W<esc(message)>".

error_reporting(-1);
ini_set('display_errors', '1');
ini_set('precision', '14');
ini_set('serialize_precision', '-1');
ini_set('default_charset', 'UTF-8');

function esc(string $s): string {
    $o = "'";
    $n = strlen($s);
    for ($i = 0; $i < $n; $i++) {
        $c = ord($s[$i]);
        if ($c <= 0x20 || $c >= 0x7f || $c == 0x25) {
            $o .= sprintf('%%%02X', $c);
        } else {
            $o .= $s[$i];
        }
    }
    return $o;
}

/** UTF-8 encoding of a code point (mbstring is not loaded with -n). */
function u8chr(int $cp): string {
    if ($cp < 0x80) return chr($cp);
    if ($cp < 0x800) return chr(0xC0 | $cp >> 6) . chr(0x80 | $cp & 0x3F);
    if ($cp < 0x10000) return chr(0xE0 | $cp >> 12) . chr(0x80 | $cp >> 6 & 0x3F) . chr(0x80 | $cp & 0x3F);
    return chr(0xF0 | $cp >> 18) . chr(0x80 | $cp >> 12 & 0x3F) . chr(0x80 | $cp >> 6 & 0x3F) . chr(0x80 | $cp & 0x3F);
}

$WARN = [];
set_error_handler(function ($no, $msg) use (&$WARN) {
    $WARN[] = $msg;
    return true;
});

$OPTS = [];      // serialize(options) => index
$CUR = null;     // serialize of current input
$CURV = null;

function opt($o): int {
    global $OPTS;
    $k = serialize($o);
    if (!isset($OPTS[$k])) {
        $OPTS[$k] = count($OPTS);
        echo 'O ', $OPTS[$k], ' ', esc($k), "\n";
    }
    return $OPTS[$k];
}

function input($v): void {
    global $CUR, $CURV;
    $s = serialize($v);
    if ($CUR !== $s) {
        $CUR = $s;
        echo is_string($v) ? 'I ' . esc($v) : 'J ' . esc($s), "\n";
    }
    $CURV = $v;
}

function result($in, callable $f): string {
    global $WARN;
    $WARN = [];
    try {
        $r = $f();
        $out = match (true) {
            $r === $in => '=',
            $r === false => 'F',
            $r === true => 'T',
            $r === null => 'N',
            default => 'R' . esc(serialize($r)),
        };
    } catch (Throwable $e) {
        $out = 'X' . get_class($e) . ':' . esc($e->getMessage());
    }
    foreach ($WARN as $w) {
        $out .= ' W' . esc($w);
    }
    return $out;
}

/** filter_var($v, $filter, $options) */
function V($v, int $filter, $options = 0): void {
    input($v);
    $n = opt($options);
    echo 'V ', $filter, ' ', $n, ' ', result($v, fn() => filter_var($v, $filter, $options)), "\n";
}

/** filter_var_array($v, $options, $add_empty) */
function A(array $v, $options, bool $add_empty = true): void {
    input($v);
    $f = is_int($options) ? (string)$options : 'o' . opt($options);
    echo 'A ', $f, ' ', (int)$add_empty, ' ', result($v, fn() => filter_var_array($v, $options, $add_empty)), "\n";
}

/** The flag subsets of $flags (bits) with at most two flags. */
function pairs(array $flags): array {
    $out = [0];
    foreach ($flags as $i => $f) {
        $out[] = $f;
        foreach ($flags as $j => $g) {
            if ($j > $i) {
                $out[] = $f | $g;
            }
        }
    }
    return $out;
}

/** Every flag subset of $flags (bits). */
function subsets(array $flags): array {
    $out = [0];
    foreach ($flags as $f) {
        foreach ($out as $o) {
            $out[] = $o | $f;
        }
    }
    return $out;
}

$bytes = [];
for ($b = 0; $b < 256; $b++) {
    $bytes[] = chr($b);
}

// ------------------------------------------------------------ char classes
// Every byte before, after and inside a valid value of each validator.
$samples = [
    FILTER_VALIDATE_INT => ['1', '12'],
    FILTER_VALIDATE_FLOAT => ['1.5', '15'],
    FILTER_VALIDATE_BOOL => ['on', '1', 'no'],
    FILTER_VALIDATE_IP => ['1.2.3.4', '::1', '1::2'],
    FILTER_VALIDATE_MAC => ['01:23:45:67:89:ab', '0123.4567.89ab'],
    FILTER_VALIDATE_EMAIL => ['ab@example.com', 'a@b.cd', '"ab"@example.com', 'a@[1.2.3.4]'],
    FILTER_VALIDATE_DOMAIN => ['ab.cd', 'ab'],
    FILTER_VALIDATE_URL => ['http://ab.cd/ef?gh#ij', 'http://u:p@ab.cd:80/', 'mailto:ab', 'ab://cd'],
];
foreach ($samples as $filter => $vals) {
    foreach ($vals as $val) {
        $n = strlen($val);
        foreach ($bytes as $c) {
            $positions = [0, $n];
            for ($p = 1; $p < $n; $p++) {
                $positions[] = $p;
            }
            foreach ($positions as $p) {
                $s = substr($val, 0, $p) . $c . substr($val, $p);
                V($s, $filter);
                if ($filter == FILTER_VALIDATE_DOMAIN) {
                    V($s, $filter, FILTER_FLAG_HOSTNAME);
                }
                if ($filter == FILTER_VALIDATE_EMAIL && $c >= "\x80") {
                    V($s, $filter, FILTER_FLAG_EMAIL_UNICODE);
                }
            }
            // replacing a character (MAC digits and IPv4 parts only)
            if ($filter == FILTER_VALIDATE_MAC || $val == '1.2.3.4') {
                for ($p = 0; $p < $n; $p++) {
                    $s = $val;
                    $s[$p] = $c;
                    V($s, $filter);
                }
            }
        }
    }
}
foreach (['1', 'on', '1.5', '1.2.3.4', 'a@b.cd', 'ab.cd', 'http://a.b/'] as $val) {
    foreach (["\t", "\n", "\r", "\v", "\f", " ", "\0"] as $c) {
        foreach (["$c$c$val$c$c", "$c$val$c", " \t\n$val\r\n "] as $s) {
            foreach ([FILTER_VALIDATE_INT, FILTER_VALIDATE_FLOAT, FILTER_VALIDATE_BOOL, FILTER_VALIDATE_IP, FILTER_VALIDATE_EMAIL, FILTER_VALIDATE_DOMAIN, FILTER_VALIDATE_URL, FILTER_VALIDATE_MAC] as $f) {
                V($s, $f);
            }
        }
    }
}

// ------------------------------------------------------------ int
$ints = ['', ' ', '0', '00', '000', '01', '07', '08', '010', '1', '-1', '+1', '-0', '+0', '-00', '--1', '+-1', '-+1', '1-', '1+',
    '12a', 'a12', '1 2', '1.0', '1.', '.1', '1e3', '1E3', '0x1A', '0X1a', '0x1g', '0x', '0X', '0xg', '-0x1', '+0x1', '0x-1', '0x+1', 'x1', '0x0', '0x00ff', '00x1',
    '0o17', '0O17', '0o', '0o8', '0o-1', '-0o1', '0o0', '0o00', 'o17', '017', '0017', '-017', '+017', '0b1', '0B1', '0b', 'ff', 'FF', '1f', '0f',
    '9223372036854775807', '9223372036854775808', '-9223372036854775808', '-9223372036854775809', '09223372036854775807',
    '99999999999999999999', '-99999999999999999999', '18446744073709551615', '18446744073709551616',
    '0x7fffffffffffffff', '0x8000000000000000', '0xffffffffffffffff', '0x10000000000000000', '0x0000000000000000001', '0xFFFFFFFFFFFFFFFF0',
    '0777777777777777777777', '01000000000000000000000', '01777777777777777777777', '02000000000000000000000', '0o1777777777777777777777', '0o2000000000000000000000',
    '00000000000000000000000000001', '0x00000000000000000000000000001', '1_000', '1,000', "\xd9\xa1", '٣', '１', '12345678901234567890123', '123abc', '0.0', '-', '+', 'e', 'x', 'o',
    '2147483647', '2147483648', '-2147483648', '-2147483649', '4294967296'];
foreach ($ints as $s) {
    foreach (subsets([FILTER_FLAG_ALLOW_OCTAL, FILTER_FLAG_ALLOW_HEX]) as $fl) {
        V($s, FILTER_VALIDATE_INT, $fl);
    }
}
$ranges = [[-5, 5], [5, -5], [0, 0], [PHP_INT_MIN, PHP_INT_MAX], [1, null], [null, 1], [-1, null], [null, -1], [255, 255], [256, 1000]];
foreach (['0', '1', '-1', '5', '-5', '6', '-6', '255', '256', '0xff', '0377', '9223372036854775807', '-9223372036854775808', 'x'] as $s) {
    foreach ($ranges as [$lo, $hi]) {
        $o = [];
        if ($lo !== null) $o['min_range'] = $lo;
        if ($hi !== null) $o['max_range'] = $hi;
        V($s, FILTER_VALIDATE_INT, ['options' => $o, 'flags' => FILTER_FLAG_ALLOW_HEX | FILTER_FLAG_ALLOW_OCTAL]);
        V($s, FILTER_VALIDATE_INT, ['options' => $o + ['default' => 'D'], 'flags' => FILTER_FLAG_ALLOW_HEX]);
    }
}
foreach ([['min_range' => '3'], ['min_range' => '3x'], ['min_range' => 'x'], ['min_range' => 2.9], ['min_range' => -2.9], ['min_range' => true], ['min_range' => null], ['min_range' => []], ['min_range' => [1]],
    ['max_range' => '3'], ['max_range' => 2.9], ['max_range' => 1e30], ['max_range' => -1e30], ['min_range' => 1e30], ['max_range' => NAN], ['min_range' => '1e3'], ['max_range' => ' 3'], ['max_range' => '3 ']] as $o) {
    foreach (['2', '3', '-3', '1000', '-1000'] as $s) {
        V($s, FILTER_VALIDATE_INT, ['options' => $o]);
    }
}

// ------------------------------------------------------------ float
$floats = ['', ' ', '0', '-0', '+0', '0.0', '-0.0', '00', '007', '00.5', '1', '1.', '.1', '.', '-.', '+.', '-.1', '+.1', '1..2', '1.2.3', '1.5', '-1.5', '+1.5', '--1.5',
    '1e5', '1E5', '1e+5', '1e-5', '1e', '1e+', '1e-', 'e5', '.e5', '1.e5', '.5e5', '5.e5', '1e5.5', '1e05', '1e+05', '1e5e5', '1ee5', '1.5e5', '-1.5E-5', '1d5', '1f',
    '1,000', '1,000.5', '1,0000', '1,00', ',100', '100,', '1,,000', '1,000,000', '12,345,678.9', '-1,000', '+1,000', '1,000e3', '1e3,000', '1,000.000,5', '1.000.000', '1.000,5', "1'000", "1'000.5", '1 000', '1,000 000',
    '0x1A', '0x1p3', 'inf', 'INF', '-INF', 'infinity', 'nan', 'NAN', '1e308', '1.7976931348623157e308', '1.7976931348623159e308', '1e309', '-1e309', '1e-308', '2.2250738585072014e-308', '4.9e-324', '2e-324', '1e-323', '1e-324', '1e-400',
    '123456789012345678901234567890', '0.1234567890123456789012345', '9007199254740993', '0.30000000000000004', '1_0', '1.5_', '١.٥', '1.5f', '1.5 ', '1,5', '1;5', '1.5.', '.5.', '1e5 ', '5e-1', '0e0', '0.e0', '.0e0'];
$fopts = [0, FILTER_FLAG_ALLOW_THOUSAND, FILTER_FLAG_ALLOW_FRACTION, FILTER_FLAG_ALLOW_SCIENTIFIC,
    ['options' => ['decimal' => ',']], ['options' => ['decimal' => ','], 'flags' => FILTER_FLAG_ALLOW_THOUSAND],
    ['options' => ['thousand' => ' '], 'flags' => FILTER_FLAG_ALLOW_THOUSAND], ['options' => ['thousand' => '.', 'decimal' => ','], 'flags' => FILTER_FLAG_ALLOW_THOUSAND],
    ['options' => ['thousand' => ',.\''], 'flags' => FILTER_FLAG_ALLOW_THOUSAND], ['options' => ['thousand' => 'x'], 'flags' => FILTER_FLAG_ALLOW_THOUSAND],
    ['options' => ['decimal' => 'e']], ['options' => ['decimal' => '1']], ['options' => ['thousand' => '1'], 'flags' => FILTER_FLAG_ALLOW_THOUSAND], ['options' => ['thousand' => 'e'], 'flags' => FILTER_FLAG_ALLOW_THOUSAND]];
foreach ($floats as $s) {
    foreach ($fopts as $o) {
        V($s, FILTER_VALIDATE_FLOAT, $o);
    }
}
$franges = [[-5.5, 5.5], [5, -5], [0, 0], [1, null], [null, 1], [-INF, INF], [NAN, null], [null, NAN], ['2', '3'], ['x', null], [1e308, null]];
foreach (['0', '1', '-1', '5.5', '-5.5', '5.6', '-5.6', '1e3', '-0', 'x', '1e400'] as $s) {
    foreach ($franges as [$lo, $hi]) {
        $o = [];
        if ($lo !== null) $o['min_range'] = $lo;
        if ($hi !== null) $o['max_range'] = $hi;
        V($s, FILTER_VALIDATE_FLOAT, ['options' => $o]);
        V($s, FILTER_VALIDATE_FLOAT, ['options' => $o + ['default' => 7]]);
    }
}
foreach ([['decimal' => ''], ['decimal' => '..'], ['decimal' => 1], ['decimal' => null], ['thousand' => ''], ['thousand' => 1], ['thousand' => []], ['decimal' => ',', 'thousand' => '']] as $o) {
    foreach (['1.5', '1,5', 'x'] as $s) {
        V($s, FILTER_VALIDATE_FLOAT, ['options' => $o]);
        V($s, FILTER_VALIDATE_FLOAT, ['options' => $o, 'flags' => FILTER_FLAG_ALLOW_THOUSAND]);
    }
}

// ------------------------------------------------------------ bool
$bools = ['', ' ', '1', '0', 'true', 'false', 'on', 'off', 'yes', 'no', 'TRUE', 'True', 'tRuE', 'FALSE', 'On', 'oN', 'OFF', 'oFf', 'Yes', 'YES', 'nO', 'NO',
    '2', '-1', '1.0', '0.0', '00', '01', '10', 'y', 'n', 't', 'f', 'truex', 'xtrue', 'none', 'null', 'ok', 'tru', 'fals', 'of', 'o', 'ye', ' on ', 'o n', "on\0", "\0on", 'ON ', ' OFF'];
foreach ($bools as $s) {
    foreach ([0, FILTER_NULL_ON_FAILURE, ['options' => ['default' => 'D']], ['options' => ['default' => 'D'], 'flags' => FILTER_NULL_ON_FAILURE], FILTER_THROW_ON_FAILURE] as $o) {
        V($s, FILTER_VALIDATE_BOOL, $o);
    }
}

// ------------------------------------------------------------ ip
$ips = ['', '1', '1.2', '1.2.3', '1.2.3.4', '1.2.3.4.5', '1.2.3.', '.1.2.3', '1..2.3', '0.0.0.0', '255.255.255.255', '256.0.0.0', '1.256.0.0', '1.2.3.256', '01.2.3.4', '1.02.3.4', '1.2.3.04', '00.0.0.0', '0.00.0.0',
    '1.2.3.4 ', ' 1.2.3.4', '1.2.3.4/24', '1.2.3.4:80', '1.2.3.0x4', '0x1.2.3.4', '1.2.3.-4', '+1.2.3.4', '1.2.3.+4', '999.1.1.1', '1000.1.1.1', '1.2.3.4a', 'a.b.c.d', '1,2,3,4', '１.2.3.4',
    '::', ':', ':::', '::1', '1::', '1::1', '1:2:3:4:5:6:7:8', '1:2:3:4:5:6:7:8:9', '1:2:3:4:5:6:7', '1:2:3:4:5:6:7::', '::1:2:3:4:5:6:7', '1::2:3:4:5:6:7', '1:2:3:4:5:6::7:8', '1:2:3:4::5:6:7:8',
    '::1:2:3:4:5:6:7:8', '1:2:3:4:5:6:7:8::', ':1:2:3:4:5:6:7:8', '1:2:3:4:5:6:7:8:', ':1::', '::1:', '1:::2', '1::2::3', 'ffff::', 'FFFF::', 'fFfF::', 'ffff0::', '0ffff::', '00000::', '0000::', '000::', '00::',
    'g::', '::g', '1:2:3:4:5:6:1.2.3.4', '::1.2.3.4', '::ffff:1.2.3.4', '1::1.2.3.4', '1:2:3:4:5:6:7:1.2.3.4', '1:2:3:4:5::1.2.3.4', '1:2:3:4:5:6::1.2.3.4', '::1.2.3', '::1.2.3.4.5', '::256.1.1.1', '::01.2.3.4',
    '1.2.3.4::', '1.2.3.4:1::', '::1.2.3.4:1', '::ffff:1.2.3.4%eth0', 'fe80::1%eth0', 'fe80::1%1', '[::1]', '::1/128', ' ::1', '::1 ', '1:2:3:4:5:6:7:8 ', '2001:db8::', '2001:DB8::1', 'fffff::', '::fffff',
    str_repeat('f', 40) . '::', '::' . str_repeat('f', 40), str_repeat('1:', 20) . '1', '1:2:3:4:5:6:7:' . str_repeat('0', 10), '0:0:0:0:0:0:0:0', '0:0:0:0:0:0:0:1', '0000:0000:0000:0000:0000:0000:0000:0001',
    '::0:0:0:0:0:0:0', '0:0:0:0:0:0:0::', '::0:0:0:0:0:0', ':0:0:0:0:0:0:0:0', '1:2:3:4:5:6:7:8.1', '::1.2.3.4.', '::.1.2.3', '1.2.3.4:', '::1:2:3:4:5:6:1.2.3.4', '1:2:3:4:5:6:7:8:1.2.3.4'];
foreach ($ips as $s) {
    foreach ([0, FILTER_FLAG_IPV4, FILTER_FLAG_IPV6, FILTER_FLAG_IPV4 | FILTER_FLAG_IPV6] as $fl) {
        V($s, FILTER_VALIDATE_IP, $fl);
    }
}
// range boundaries
function v4add(string $ip, int $d): ?string {
    $n = ip2long($ip) + $d;
    if ($n < 0 || $n > 0xffffffff) return null;
    return long2ip($n);
}
function v6add(string $bin, int $d): ?string {
    $b = array_values(unpack('C*', $bin));
    $i = 15;
    if ($d > 0) {
        while ($i >= 0) { if ($b[$i] == 255) { $b[$i] = 0; $i--; } else { $b[$i]++; break; } }
        if ($i < 0) return null;
    } else {
        while ($i >= 0) { if ($b[$i] == 0) { $b[$i] = 255; $i--; } else { $b[$i]--; break; } }
        if ($i < 0) return null;
    }
    return pack('C*', ...$b);
}
function v6range(string $cidr): array {
    [$a, $len] = explode('/', $cidr);
    $bin = inet_pton($a);
    $lo = '';
    $hi = '';
    for ($i = 0; $i < 16; $i++) {
        $bits = max(0, min(8, $len - $i * 8));
        $mask = $bits == 0 ? 0 : (0xff << (8 - $bits)) & 0xff;
        $lo .= chr(ord($bin[$i]) & $mask);
        $hi .= chr((ord($bin[$i]) & $mask) | (~$mask & 0xff));
    }
    return [$lo, $hi];
}
$v4blocks = ['0.0.0.0/8', '10.0.0.0/8', '100.64.0.0/10', '127.0.0.0/8', '169.254.0.0/16', '172.16.0.0/12', '192.0.0.0/24', '192.0.0.0/29', '192.0.0.8/32', '192.0.0.9/32', '192.0.0.10/32',
    '192.0.0.170/31', '192.0.2.0/24', '192.31.196.0/24', '192.52.193.0/24', '192.88.99.0/24', '192.168.0.0/16', '192.175.48.0/24', '198.18.0.0/15', '198.51.100.0/24', '203.0.113.0/24',
    '224.0.0.0/4', '240.0.0.0/4', '255.255.255.255/32', '1.0.0.0/8', '128.0.0.0/16', '191.255.0.0/16', '223.255.255.0/24', '100.0.0.0/8', '172.0.0.0/8'];
$v4flags = [0, FILTER_FLAG_NO_PRIV_RANGE, FILTER_FLAG_NO_RES_RANGE, FILTER_FLAG_GLOBAL_RANGE, FILTER_FLAG_NO_PRIV_RANGE | FILTER_FLAG_NO_RES_RANGE, FILTER_FLAG_IPV4 | FILTER_FLAG_GLOBAL_RANGE, FILTER_FLAG_IPV6 | FILTER_FLAG_NO_RES_RANGE];
foreach ($v4blocks as $blk) {
    [$a, $len] = explode('/', $blk);
    $lo = ip2long($a) & (($len == 0) ? 0 : (0xffffffff << (32 - $len)) & 0xffffffff);
    $hi = $lo | ((1 << (32 - $len)) - 1);
    foreach ([long2ip($lo), long2ip($hi), v4add(long2ip($lo), -1), v4add(long2ip($hi), 1), long2ip($lo + intdiv($hi - $lo, 2))] as $ip) {
        if ($ip === null) continue;
        foreach ($v4flags as $fl) {
            V($ip, FILTER_VALIDATE_IP, $fl);
        }
        foreach ([FILTER_FLAG_NO_PRIV_RANGE, FILTER_FLAG_NO_RES_RANGE, FILTER_FLAG_GLOBAL_RANGE] as $fl) {
            V("::ffff:$ip", FILTER_VALIDATE_IP, $fl);
            V("::$ip", FILTER_VALIDATE_IP, $fl);
        }
    }
}
$v6blocks = ['::/128', '::1/128', '::/96', '::ffff:0:0/96', '64:ff9b::/96', '64:ff9b:1::/48', '100::/64', '2001::/23', '2001::/32', '2001:1::1/128', '2001:1::2/128', '2001:1::3/128', '2001:2::/48', '2001:3::/32',
    '2001:4:112::/48', '2001:10::/28', '2001:20::/28', '2001:30::/28', '2001:db8::/32', '2002::/16', '2620:4f:8000::/48', 'fc00::/7', 'fe80::/10', 'fec0::/10', 'ff00::/8', '5f00::/16', '3fff::/20', '3ffe::/16',
    '::/8', '100::/8', '200::/7', '400::/6', '800::/5', '1000::/4', '4000::/3', '6000::/3', '8000::/3', 'a000::/3', 'c000::/3', 'e000::/4', 'f000::/5', 'f800::/6', 'fe00::/9', '2000::/3', '::ffff:0:0:0/96', '::0.0.0.0/96'];
$v6flags = [0, FILTER_FLAG_NO_PRIV_RANGE, FILTER_FLAG_NO_RES_RANGE, FILTER_FLAG_GLOBAL_RANGE, FILTER_FLAG_IPV6 | FILTER_FLAG_NO_PRIV_RANGE | FILTER_FLAG_NO_RES_RANGE];
foreach ($v6blocks as $blk) {
    [$lo, $hi] = v6range($blk);
    $cands = [$lo, $hi, v6add($lo, -1), v6add($hi, 1), v6add($lo, 1), v6add($hi, -1)];
    foreach ($cands as $bin) {
        if ($bin === null) continue;
        $ip = inet_ntop($bin);
        $full = implode(':', str_split(bin2hex($bin), 4));
        foreach (array_unique([$ip, $full]) as $s) {
            foreach ($v6flags as $fl) {
                V($s, FILTER_VALIDATE_IP, $fl);
            }
        }
    }
}

// ------------------------------------------------------------ mac
$macs = ['01:23:45:67:89:ab', '01-23-45-67-89-ab', '0123.4567.89ab', '01:23:45:67:89:AB', '01:23:45:67:89', '01:23:45:67:89:ab:cd', '01:23:45:67:89:a', '01:23:45:67:89:abc', '1:23:45:67:89:ab',
    '01:23:45-67:89:ab', '01-23-45-67-89:ab', '0123.4567.89a', '0123.4567.89abc', '0123.4567', '0123.4567.89ab.cdef', '0123:4567:89ab', '0123-4567-89ab', '01.23.45.67.89.ab', '012345678 9ab', '0123456789ab',
    'gg:23:45:67:89:ab', '01:23:45:67:89:ag', '01:23:45:67:89:ab ', ' 01:23:45:67:89:ab', '01::45:67:89:ab', '::::::', '', '00:00:00:00:00:00', 'ff:ff:ff:ff:ff:ff', 'FFFF.FFFF.FFFF', '01:23:45:67:89:ab:', ':01:23:45:67:89:ab',
    '0x:23:45:67:89:ab', '+1:23:45:67:89:ab', '-1:23:45:67:89:ab', '01:23:45:67:89:-b', '01:23:45:67:89: b', '0123.4567.89 b', '0123.4567.-9ab', '0123.4567.89aB'];
$macopts = [0, ['options' => ['separator' => ':']], ['options' => ['separator' => '-']], ['options' => ['separator' => '.']], ['options' => ['separator' => 'x']], ['options' => ['separator' => ' ']],
    ['options' => ['separator' => '']], ['options' => ['separator' => '::']], ['options' => ['separator' => 1]], ['options' => ['separator' => null]]];
foreach ($macs as $s) {
    foreach ($macopts as $o) {
        V($s, FILTER_VALIDATE_MAC, $o);
    }
}

// ------------------------------------------------------------ domain
$l63 = str_repeat('a', 63);
$l64 = str_repeat('a', 64);
$domains = ['', '.', '..', 'a', 'a.', '.a', 'a..b', 'a.b', 'a.b.', 'a.b..', 'example.com', 'EXAMPLE.COM', '1.2.3.4', '123', '123.456', 'a-b.c', '-a.b', 'a-.b', 'a.-b', 'a.b-', 'a--b.c', 'xn--a.b', '_a.b', 'a_.b', 'a._b', 'a.b_', 'a_b.c',
    'a b.c', 'a.b c', "a\tb", 'a/b', 'a@b', 'a:b', 'ä.com', 'a*.com', '*.a.com', 'a%b.c', "$l63.com", "$l64.com", "a.$l63", "a.$l64", $l63, $l64, "$l63.", "$l64.",
    implode('.', [$l63, $l63, $l63, str_repeat('a', 61)]), implode('.', [$l63, $l63, $l63, str_repeat('a', 62)]), implode('.', [$l63, $l63, $l63, str_repeat('a', 61)]) . '.', implode('.', [$l63, $l63, $l63, str_repeat('a', 62)]) . '.',
    implode('.', [$l63, $l63, $l63, str_repeat('a', 60)]) . '.', implode('.', [$l63, $l63, $l63, str_repeat('a', 63)]), str_repeat('a.', 126) . 'a', str_repeat('a.', 127), str_repeat('a.', 127) . 'a', str_repeat('a.', 126) . 'ab',
    '-', '_', 'a-', '-a', '0', '0-0', '9a.b', 'a.9', 'a.9b', 'a.b.9', 'a.b-c', 'aa.bb.cc.dd', 'localhost', 'a.b.c.', ' a.b', 'a.b ', "a\0b"];
foreach ($domains as $s) {
    V($s, FILTER_VALIDATE_DOMAIN);
    V($s, FILTER_VALIDATE_DOMAIN, FILTER_FLAG_HOSTNAME);
}

// ------------------------------------------------------------ url
$urls = ['http://example.com', 'http://example.com/', 'http://example.com/p', 'http://example.com?q', 'http://example.com#f', 'http://example.com/p?q#f', 'https://example.com', 'HTTP://example.com', 'Http://Example.COM',
    'ftp://example.com', 'ftp://ex_ample.com', 'foo://ex_ample.com', 'foo://-a.com', 'http://-a.com', 'https://a_b.com', 'HTTPS://a_b.com', 'httpx://a_b.com', 'file:///etc/passwd', 'file://host/etc', 'file:/etc', 'file:', 'file://',
    'mailto:a@b.c', 'mailto:', 'mailto:a b', 'news:comp.lang', 'news:', 'foo:bar', 'foo:', 'foo:/bar', 'foo://bar', 'foo://', 'foo:///bar', 'a+b-c.d://host', '1abc://host', 'a_b://host', '://host', '//host', '//host/p', '/p', 'p', '?q', '#f',
    'http:', 'http:/', 'http://', 'http:///', 'http:///p', 'http://:80', 'http://:80/', 'http://@host', 'http://u@host', 'http://u:@host', 'http://:p@host', 'http://u:p@host', 'http://u:p:q@host', 'http://u@p@host', 'http://u@host@host',
    'http://host:', 'http://host:/', 'http://host:0', 'http://host:80', 'http://host:65535', 'http://host:65536', 'http://host:99999', 'http://host:999999', 'http://host:-1', 'http://host:+1', 'http://host:1a', 'http://host:a', 'http://host:080', 'http://host:8 0',
    'http://host:80:80', 'http://[::1]', 'http://[::1]:80', 'http://[::1]/', 'http://[::1', 'http://::1]', 'http://[1.2.3.4]', 'http://[::ffff:1.2.3.4]', 'http://[v1.x]', 'http://[fe80::1%25eth0]', 'http://[::g]', 'http://[]', 'http://[',
    'http://1.2.3.4', 'http://256.256.256.256', 'http://1.2.3', 'http://0x7f.1', 'http://a', 'http://a.', 'http://.a', 'http://a..b', 'http://a.-b', 'http://a.b-', 'http://xn--a.com', 'http://a b.com', 'http://a%20b.com', 'http://ä.com',
    'http://' . $l63 . '.com', 'http://' . $l64 . '.com', 'http://host/a b', 'http://host/a%20b', 'http://host/a\\b', 'http://host/a|b', 'http://host/a^b', 'http://host/a{b}', 'http://host/a"b', 'http://host/<>', 'http://host/a`b', 'http://host//a',
    'http://host/./..', 'http://host/~u', "http://host/!$&'()*+,;=:@", 'http://host/ä', 'http://host?a b', 'http://host?a|b', 'http://host?a?b', 'http://host?/', 'http://host?[]', 'http://host#a b', 'http://host#a#b', 'http://host#',
    'http://host?', 'http://host/?', 'http://host/#', 'http://u%41@host', 'http://u;x@host', 'http://u\\x@host', 'http://u[x@host', 'http://u]x@host', "http://!$&'()*+,;=@host", 'http://-._~@host', 'http://u:p%41@host', 'http://u:p\\@host',
    'http://u:p[@host', 'http://u%@host', 'http://u%4@host', 'http://u%zz@host', 'http://host%41.com', 'http://host/%zz', 'http://host/%', 'http://host?%', 'http://host#%', "http://host/\x7f", "http://host/\0", "http://host/\t",
    'http://host\\path', 'http:\\\\host', 'javascript:alert(1)', 'data:text/plain,abc', 'urn:isbn:0451450523', 'tel:+1-816-555-1212', 'ldap://[2001:db8::7]/c=GB?objectClass?one', 'telnet://192.0.2.16:80/', 'svn+ssh://host/p',
    'http://example.com./', 'http://example.com..', 'http://a.b.c.d.e.f.g', 'http://1', 'http://12345', 'http://localhost', 'http://localhost:8080', 'http://*.com', 'http://a*.com', 'http://_.com', 'http://a_', 'foo://a_',
    'mailto://a_b', 'news://a_b', 'file://a_b', 'git://a_b', 'irc://-a', 'ssh://u@-a', 'http://user@[::1]', 'http://[::1]@host', 'http://h/p?q#f#g', 'http://h:80?q', 'http://h:80#f', 'http:h', 'http:h/p', 'http:/h', 'https:', 'https://'];
foreach ($urls as $s) {
    foreach ([0, FILTER_FLAG_PATH_REQUIRED, FILTER_FLAG_QUERY_REQUIRED, FILTER_FLAG_PATH_REQUIRED | FILTER_FLAG_QUERY_REQUIRED] as $fl) {
        V($s, FILTER_VALIDATE_URL, $fl);
    }
}
// component variations
$schemes = ['http', 'https', 'ftp', 'foo', 'mailto', 'news', 'file'];
$users = ['', 'u@', 'u:p@', ':p@', 'u:@', '@', 'u%20@', 'u p@', "u'@", 'u"@', 'u<@', 'u!@', 'u$@', 'u&@', 'u(@', 'u*@', 'u+@', 'u,@', 'u;@', 'u=@', 'u-@', 'u.@', 'u_@', 'u~@', 'u/@', 'u?@', 'u#@', 'u[@', 'u]@', 'u{@', 'u|@', 'u^@', 'u`@', 'u\\@'];
$hosts = ['example.com', 'a', 'a_b', '-a', 'a-', 'a.b.', '[::1]', '1.2.3.4', 'ex%41mple.com', 'a~b', 'a!b', "a'b", 'a(b', 'a*b', 'a+b', 'a,b', 'a;b', 'a=b', 'a$b', 'a&b', '1a', 'a.1', '0.0.0.0'];
foreach ($schemes as $sc) {
    foreach ($users as $u) {
        V("$sc://{$u}example.com/", FILTER_VALIDATE_URL);
    }
    foreach ($hosts as $h) {
        V("$sc://$h/", FILTER_VALIDATE_URL);
        V("$sc://$h", FILTER_VALIDATE_URL);
        V("$sc://u@$h:1/", FILTER_VALIDATE_URL);
    }
}

// ------------------------------------------------------------ email
$l64 = str_repeat('a', 64);
$l65 = str_repeat('a', 65);
$emails = ['', '@', 'a', 'a@', '@a', 'a@b', 'a@b.c', 'a@b.cd', 'a@b.c.d', 'a.b@c.de', '.a@b.cd', 'a.@b.cd', 'a..b@c.de', 'a@.b.cd', 'a@b..cd', 'a@b.cd.', 'a@-b.cd', 'a@b-.cd', 'a@b.-cd', 'a@b.cd-', 'a@b--c.de', 'a@b_c.de', 'a@b.c_d',
    'a@1.2.3.4', 'a@b.12', 'a@b.1c', 'a@b.c1', 'a@123.com', 'a@b.123', 'a@[1.2.3.4]', 'a@[256.1.1.1]', 'a@[1.2.3]', 'a@[01.2.3.4]', 'a@[IPv6:::1]', 'a@[IPv6:1::2]', 'a@[ipv6:::1]', 'a@[IPV6:::1]', 'a@[IPv6:1:2:3:4:5:6:7:8]', 'a@[IPv6:1:2:3:4:5:6:7:8:9]',
    'a@[IPv6:1:2:3:4:5:6:1.2.3.4]', 'a@[IPv6:::ffff:1.2.3.4]', 'a@[IPv6:g::]', 'a@[::1]', 'a@[foo]', 'a@[a:b]', 'a@[1.2.3.4', 'a@1.2.3.4]', 'a@[]', 'a@[1.2.3.4].com', 'a@b.[1.2.3.4]',
    '"a"@b.cd', '""@b.cd', '"a b"@b.cd', '"a\\"b"@b.cd', '"a\\\\b"@b.cd', '"a\\b"@b.cd', '"a"b"@b.cd', '"a@b"@c.de', '"a"."b"@c.de', '"a".b@c.de', 'a."b"@c.de', 'a"b"@c.de', '"a"b@c.de', '"a@b.cd', 'a"@b.cd', '"a\\"@b.cd',
    '"\\a"@b.cd', "\"a\tb\"@b.cd", "\"a\nb\"@b.cd", "\"a\rb\"@b.cd", "\"a\0b\"@b.cd", '"a\\ b"@b.cd', '"(a)"@b.cd', '"a.."@b.cd', '".a"@b.cd', '"a."@b.cd',
    'a(b)@c.de', '(a)b@c.de', 'a@(b)c.de', 'a@c.de(x)', 'a <b@c.de>', '<a@b.cd>', 'a@b.cd ', ' a@b.cd', 'a @b.cd', 'a@ b.cd', 'a@b@c.de', "a!#$%&'*+-/=?^_`{|}~@b.cd", 'a,b@c.de', 'a:b@c.de', 'a;b@c.de', 'a[b@c.de', 'a]b@c.de', 'a\\b@c.de',
    "$l64@b.cd", "$l65@b.cd", "a@$l63.cd", "a@$l64.cd", 'a@' . implode('.', [$l63, $l63, $l63, str_repeat('a', 57)]), 'a@' . implode('.', [$l63, $l63, $l63, str_repeat('a', 58)]), 'a@' . implode('.', [$l63, $l63, $l63, str_repeat('a', 59)]),
    'a@' . implode('.', [$l63, $l63, $l63, str_repeat('a', 61)]), 'a@' . implode('.', [$l63, $l63, $l63, str_repeat('a', 62)]), str_repeat('a', 64) . '@' . implode('.', [$l63, $l63, str_repeat('a', 60)]), str_repeat('a', 64) . '@' . implode('.', [$l63, $l63, str_repeat('a', 61)]),
    str_repeat('a', 64) . '@' . implode('.', [$l63, $l63, str_repeat('a', 62)]), str_repeat('a', 64) . '@' . implode('.', [$l63, $l63, str_repeat('a', 63)]), '"' . str_repeat('a', 62) . '"@b.cd', '"' . str_repeat('a', 63) . '"@b.cd',
    'A@B.CD', 'a@B.cD', 'a@b.c-d', 'a@xn--b.cd', 'a@b.xn--cd', 'ä@b.cd', 'a@ä.cd', 'a@b.ä', 'äb@c.de', "\xc3@b.cd", "\xff@b.cd", "a\xc3\xa4b@c.de", "\xe2\x82\xac@b.cd", "\xf0\x9f\x98\x80@b.cd", "\xed\xa0\x80@b.cd", "\xc0\x80@b.cd", "\xe0\x80\x80@b.cd",
    "\xf4\x90\x80\x80@b.cd", "\xf0\x80\x80\x80@b.cd", '"ä"@b.cd', "\"\xff\"@b.cd", 'a@b.cd' . "\n", "a@b.cd\r\n", 'test@example.com', 'test.test@example.com', 'test+tag@example.com', 'x@example', 'x@localhost', 'x@a.b.c.d.e.f',
    'a@b.c' . str_repeat('d', 62), 'a@b.c' . str_repeat('d', 63), 'a@b.co', 'a@b.' . str_repeat('c', 64), 'a@0.0', 'a@0.a', 'a@a.0', 'a@a0.b', 'a@-.b', 'a@a.b-c-d', '-@b.cd', '-a@b.cd', 'a-@b.cd', '.@b.cd', '..@b.cd', '_@b.cd',
    'a@[1.2.3.4]x', 'a@[IPv6:1:2:3:4:5:6:7]', 'a@[IPv6:1:2:3:4:5:6:7::]', 'a@[IPv6:::1:2:3:4:5:6:7]', 'a@[IPv6:1::2::3]', 'a@[IPv6:12345::]', 'a@[IPv6:1:2:3:4:5::1.2.3.4]', 'a@[IPv6:1:2:3:4:5:6:7:1.2.3.4]', 'a@[IPv6:::1.2.3]',
    'a@[ 1.2.3.4]', 'a@[1.2.3.4 ]', 'a@[\\1.2.3.4]', 'a@[x:1.2.3.4]', 'a@[0.0.0.0]', 'a@[255.255.255.255]', 'a@[1.2.3.04]', 'a@[IPv6:]', 'a@[IPv6:::]', 'a@[IPv6::1]', 'a@[IPv6:1:]'];
foreach ($emails as $s) {
    V($s, FILTER_VALIDATE_EMAIL);
    V($s, FILTER_VALIDATE_EMAIL, FILTER_FLAG_EMAIL_UNICODE);
}
// combinations of local parts and domains
$locals = ['a', 'a.b', 'a+b', '"a"', '"a b"', 'a..b', '.a', 'a.', '"a\\"b"', 'ä', '#', "'", '{}', '!', '~', '`', '|', '/', '=', '?', '^', '"a".b', 'a.b.c.d', '0', 'A-Z', '"."', '"@"', '"\\\\"'];
$doms = ['b.cd', 'b', 'b.c', 'b.c1', 'b.1c', 'b.123', '1.2', 'b-c.de', '[1.2.3.4]', '[IPv6:::1]', 'b.cd.', 'xn--b.cd', 'b.xn--c', 'B.CD', 'b.c.d.e', '9.cd', 'b.9d', 'b.d9', 'b.c-d', 'b.-cd', 'b.cd-', 'b..cd', 'ä.cd'];
foreach ($locals as $lp) {
    foreach ($doms as $d) {
        V("$lp@$d", FILTER_VALIDATE_EMAIL);
        V("$lp@$d", FILTER_VALIDATE_EMAIL, FILTER_FLAG_EMAIL_UNICODE);
    }
}
// local part dot structures over a small alphabet
foreach (['a', '.', '"', '\\', ' '] as $c1) {
    foreach (['a', '.', '"', '\\', ' ', ''] as $c2) {
        foreach (['a', '.', '"', '\\', ' ', ''] as $c3) {
            foreach (['a', '.', '"', '\\', ''] as $c4) {
                V("$c1$c2$c3$c4@b.cd", FILTER_VALIDATE_EMAIL);
            }
        }
    }
}

// ------------------------------------------------------------ regexp
$res = ['/^a/', '/a$/', '/^a$/', '/a/i', '/^.$/s', '/^.$/', '/^\d+$/', '/x/', '/^$/', '/^a/m', '~a~', '#^\w+$#u', '/(?<n>a)/', '/(/', 'abc', '', '/a/Z', "/a\0/", '/\\x00/', '/./u', '/^\p{L}+$/u'];
foreach ($res as $re) {
    foreach (['a', 'A', "b\na", "a\n", '123', '', 'é', "\xff", "a\0"] as $s) {
        V($s, FILTER_VALIDATE_REGEXP, ['options' => ['regexp' => $re]]);
    }
}
foreach ([0, ['options' => []], ['options' => ['regexp' => 1]], ['options' => ['regexp' => null]], ['options' => 'x'], ['options' => ['regexp' => '/(/', 'default' => 'D']], ['options' => ['regexp' => '/x/', 'default' => 'D'], 'flags' => FILTER_NULL_ON_FAILURE]] as $o) {
    V('abc', FILTER_VALIDATE_REGEXP, $o);
    V(['abc'], FILTER_VALIDATE_REGEXP, $o);
}

// ------------------------------------------------------------ sanitizers
$strflags = [FILTER_FLAG_STRIP_LOW, FILTER_FLAG_STRIP_HIGH, FILTER_FLAG_STRIP_BACKTICK, FILTER_FLAG_ENCODE_LOW, FILTER_FLAG_ENCODE_HIGH, FILTER_FLAG_ENCODE_AMP];
$sanitizers = [FILTER_SANITIZE_STRING => array_merge(pairs($strflags), [FILTER_FLAG_NO_ENCODE_QUOTES, FILTER_FLAG_NO_ENCODE_QUOTES | FILTER_FLAG_ENCODE_AMP, array_sum($strflags)]),
    FILTER_SANITIZE_ENCODED => pairs([FILTER_FLAG_STRIP_LOW, FILTER_FLAG_STRIP_HIGH, FILTER_FLAG_STRIP_BACKTICK, FILTER_FLAG_ENCODE_LOW, FILTER_FLAG_ENCODE_HIGH]),
    FILTER_SANITIZE_SPECIAL_CHARS => pairs([FILTER_FLAG_STRIP_LOW, FILTER_FLAG_STRIP_HIGH, FILTER_FLAG_STRIP_BACKTICK, FILTER_FLAG_ENCODE_HIGH, FILTER_FLAG_ENCODE_LOW]),
    FILTER_SANITIZE_FULL_SPECIAL_CHARS => [0, FILTER_FLAG_NO_ENCODE_QUOTES, FILTER_FLAG_STRIP_LOW | FILTER_FLAG_STRIP_HIGH | FILTER_FLAG_ENCODE_HIGH | FILTER_FLAG_ENCODE_LOW | FILTER_FLAG_ENCODE_AMP | FILTER_FLAG_STRIP_BACKTICK],
    FILTER_UNSAFE_RAW => array_merge(pairs($strflags), [array_sum($strflags)]),
    FILTER_SANITIZE_EMAIL => [0, FILTER_FLAG_STRIP_LOW],
    FILTER_SANITIZE_URL => [0, FILTER_FLAG_STRIP_LOW],
    FILTER_SANITIZE_NUMBER_INT => [0, FILTER_FLAG_ALLOW_FRACTION],
    FILTER_SANITIZE_NUMBER_FLOAT => subsets([FILTER_FLAG_ALLOW_FRACTION, FILTER_FLAG_ALLOW_THOUSAND, FILTER_FLAG_ALLOW_SCIENTIFIC]),
    FILTER_SANITIZE_ADD_SLASHES => [0, FILTER_FLAG_STRIP_LOW]];
foreach ($sanitizers as $f => $flagsets) {
    foreach ($flagsets as $fl) {
        foreach ($bytes as $c) {
            V("a{$c}b", $f, $fl);
        }
        V('', $f, $fl);
        V('', $f, $fl | FILTER_FLAG_EMPTY_STRING_NULL);
        V("\x01", $f, $fl | FILTER_FLAG_EMPTY_STRING_NULL);
        V('<b>', $f, $fl | FILTER_FLAG_EMPTY_STRING_NULL);
        V("é<\"'&amp;`\x01\x7f\xff", $f, $fl);
    }
}
// tag stripping of the string filter
$alpha = ['a', '<', '>', '"', "'"];
$strs = [''];
for ($len = 1; $len <= 5; $len++) {
    $next = [];
    foreach ($strs as $s) {
        if (strlen($s) == $len - 1) {
            foreach ($alpha as $c) {
                $next[] = $s . $c;
            }
        }
    }
    foreach ($next as $s) {
        V($s, FILTER_SANITIZE_STRING, FILTER_FLAG_NO_ENCODE_QUOTES);
    }
    $strs = $next;
}
mt_srand(42);
$alpha2 = ['a', 'b', '<', '>', '"', "'", ' ', "\n", '!', '-', '?', '/', '=', '&', '`', "\0", "\x80", 'é'];
for ($i = 0; $i < 3000; $i++) {
    $s = '';
    $n = mt_rand(1, 14);
    for ($j = 0; $j < $n; $j++) {
        $s .= $alpha2[mt_rand(0, count($alpha2) - 1)];
    }
    V($s, FILTER_SANITIZE_STRING, $i % 3 == 0 ? 0 : ($i % 3 == 1 ? FILTER_FLAG_NO_ENCODE_QUOTES : FILTER_FLAG_STRIP_LOW | FILTER_FLAG_ENCODE_HIGH));
    if ($i < 500) {
        V($s, FILTER_SANITIZE_FULL_SPECIAL_CHARS, $i % 2 ? 0 : FILTER_FLAG_NO_ENCODE_QUOTES);
        V($s, FILTER_SANITIZE_SPECIAL_CHARS, $i % 2 ? 0 : FILTER_FLAG_ENCODE_HIGH);
    }
}
foreach (['<p>Let me <font color="#000000">see</font> you</p>', '<<a>>b', 'a<!-- c -->b', 'a<?php echo 1; ?>b', 'a<script>x</script>b', '<a href="x>y">z</a>', "<a href='x>y'>z</a>", 'a<b', 'a>b', '1 < 2 > 0', '< a>', '<a\nb>c',
    "\xe9t\xe9 <b>&amp;</b>", 'a<"b>c"d>e', "a<'b>c'd>e", 'a<b"c>d', '<<<>>>x', '<>', 'x<>y'] as $s) {
    foreach ([0, FILTER_FLAG_NO_ENCODE_QUOTES, FILTER_FLAG_ENCODE_AMP, FILTER_FLAG_ENCODE_HIGH | FILTER_FLAG_STRIP_LOW] as $fl) {
        V($s, FILTER_SANITIZE_STRING, $fl);
    }
    V($s, FILTER_SANITIZE_FULL_SPECIAL_CHARS);
}
foreach (['123abc', '-1.5e+3', '+-1,000.5E-3', '1.2.3', 'abc', '١٢٣', '0x1A', '1_000', ' 1 2 '] as $s) {
    foreach (subsets([FILTER_FLAG_ALLOW_FRACTION, FILTER_FLAG_ALLOW_THOUSAND, FILTER_FLAG_ALLOW_SCIENTIFIC]) as $fl) {
        V($s, FILTER_SANITIZE_NUMBER_FLOAT, $fl);
        V($s, FILTER_SANITIZE_NUMBER_INT, $fl);
    }
}

// ------------------------------------------------------------ filter_var semantics
$vals = [null, true, false, 0, 1, -1, 42, PHP_INT_MAX, PHP_INT_MIN, 0.0, -0.0, 1.5, -1.5, 0.1, 1e15, 1e20, 1e-7, 123456789012345.678, INF, -INF, NAN, '', '0', '1', 'x', '42', ' 42 ', '1.5',
    [], [1], ['a' => '1', 'b' => 'x'], [[1, 'x'], 'y'], [null, true, 1.5], new stdClass];
$flagsets = [0, FILTER_REQUIRE_SCALAR, FILTER_REQUIRE_ARRAY, FILTER_FORCE_ARRAY, FILTER_NULL_ON_FAILURE, FILTER_THROW_ON_FAILURE, FILTER_REQUIRE_ARRAY | FILTER_NULL_ON_FAILURE, FILTER_FORCE_ARRAY | FILTER_NULL_ON_FAILURE,
    FILTER_REQUIRE_ARRAY | FILTER_THROW_ON_FAILURE, FILTER_REQUIRE_SCALAR | FILTER_REQUIRE_ARRAY, FILTER_FORCE_ARRAY | FILTER_REQUIRE_SCALAR, FILTER_FORCE_ARRAY | FILTER_REQUIRE_ARRAY, FILTER_FLAG_EMPTY_STRING_NULL,
    FILTER_NULL_ON_FAILURE | FILTER_THROW_ON_FAILURE, FILTER_FORCE_ARRAY | FILTER_THROW_ON_FAILURE];
foreach ([FILTER_DEFAULT, FILTER_VALIDATE_INT, FILTER_VALIDATE_FLOAT, FILTER_VALIDATE_BOOL, FILTER_VALIDATE_EMAIL, FILTER_SANITIZE_NUMBER_INT, FILTER_SANITIZE_SPECIAL_CHARS, FILTER_SANITIZE_STRING, FILTER_SANITIZE_ENCODED, 9999, 0, -1] as $f) {
    foreach ($vals as $v) {
        foreach ($flagsets as $fl) {
            V($v, $f, $fl);
        }
        V($v, $f, ['options' => ['default' => 'D']]);
        V($v, $f, ['options' => ['default' => 'D'], 'flags' => FILTER_REQUIRE_ARRAY]);
        V($v, $f, ['options' => ['default' => 'D'], 'flags' => FILTER_THROW_ON_FAILURE]);
        V($v, $f, ['options' => ['default' => 'D'], 'flags' => FILTER_FORCE_ARRAY]);
    }
}
foreach ([['flags' => '2'], ['flags' => 2.5], ['flags' => 'x'], ['flags' => null], ['flags' => true], ['flags' => [2]], ['flags' => '0x2'], ['flags' => ' 2'], ['flags' => '2 '], ['options' => 5], ['options' => null],
    ['filter' => FILTER_VALIDATE_EMAIL], ['flags' => FILTER_FLAG_ALLOW_HEX, 'extra' => 1], ['flags' => -1], ['flags' => PHP_INT_MAX], ['flags' => 1e30], ['options' => ['default' => null]], ['options' => ['default' => [1]]], [], [1, 2]] as $o) {
    foreach (['0x10', 'x', ['0x10']] as $v) {
        V($v, FILTER_VALIDATE_INT, $o);
    }
}
// filter_var_array
$data = ['a' => '1', 'b' => 'x', 'c' => ['2', 'y', ['3']], 'd' => null, 'e' => 1.5, 'f' => true, 'g' => '', 0 => '9', 'h' => new stdClass];
foreach ([FILTER_DEFAULT, FILTER_VALIDATE_INT, FILTER_VALIDATE_BOOL, FILTER_SANITIZE_NUMBER_INT, 9999, -1, FILTER_VALIDATE_INT | FILTER_NULL_ON_FAILURE, FILTER_VALIDATE_REGEXP] as $f) {
    A($data, $f);
    A([], $f);
    A(['x' => ['y' => ['z' => '1']]], $f);
}
$specs = [
    ['a' => FILTER_VALIDATE_INT, 'b' => FILTER_VALIDATE_INT, 'zz' => FILTER_VALIDATE_INT],
    ['c' => FILTER_VALIDATE_INT],
    ['c' => ['filter' => FILTER_VALIDATE_INT, 'flags' => FILTER_REQUIRE_ARRAY]],
    ['c' => ['filter' => FILTER_VALIDATE_INT, 'flags' => FILTER_FORCE_ARRAY]],
    ['a' => ['filter' => FILTER_VALIDATE_INT, 'flags' => FILTER_FORCE_ARRAY]],
    ['a' => ['filter' => FILTER_VALIDATE_INT, 'flags' => FILTER_REQUIRE_ARRAY | FILTER_NULL_ON_FAILURE]],
    ['b' => ['filter' => FILTER_VALIDATE_INT, 'options' => ['default' => 5]]],
    ['b' => ['filter' => FILTER_VALIDATE_INT, 'flags' => FILTER_THROW_ON_FAILURE]],
    ['zz' => ['filter' => FILTER_VALIDATE_INT, 'flags' => FILTER_THROW_ON_FAILURE | FILTER_NULL_ON_FAILURE, 'options' => ['default' => 1]]],
    ['a' => ['filter' => FILTER_VALIDATE_INT, 'flags' => FILTER_THROW_ON_FAILURE | FILTER_NULL_ON_FAILURE]],
    ['a' => ['flags' => FILTER_FORCE_ARRAY]],
    ['a' => []],
    ['a' => ['filter' => '257']],
    ['a' => ['filter' => 'int']],
    ['a' => ['filter' => 9999]],
    ['a' => ['filter' => null]],
    ['a' => ['filter' => 1.5]],
    ['a' => ['filter' => FILTER_VALIDATE_INT, 'flags' => '2']],
    ['a' => null, 'b' => true, 'c' => false, 'd' => 257, 'e' => '257', 'f' => 2.5, 'g' => [], 'h' => 257],
    ['a' => 9999, 'zz' => 9999],
    ['' => FILTER_DEFAULT],
    ['a' => FILTER_DEFAULT, '' => FILTER_DEFAULT],
    ['zz' => FILTER_DEFAULT, '' => FILTER_DEFAULT],
    [0 => FILTER_DEFAULT],
    ['a' => FILTER_DEFAULT, 0 => FILTER_DEFAULT],
    ['0' => FILTER_DEFAULT],
    ['01' => FILTER_DEFAULT, '-0' => FILTER_DEFAULT, ' 0' => FILTER_DEFAULT],
    ['h' => FILTER_VALIDATE_INT, 'g' => FILTER_VALIDATE_INT, 'd' => FILTER_VALIDATE_INT, 'e' => FILTER_VALIDATE_INT, 'f' => FILTER_VALIDATE_INT],
    ['h' => ['flags' => FILTER_THROW_ON_FAILURE]],
    ['c' => ['flags' => FILTER_THROW_ON_FAILURE]],
    ['c' => ['filter' => FILTER_VALIDATE_REGEXP]],
    ['zz' => ['filter' => FILTER_VALIDATE_REGEXP]],
    ['g' => ['flags' => FILTER_FLAG_EMPTY_STRING_NULL]],
    ['a' => ['filter' => FILTER_VALIDATE_FLOAT, 'options' => ['decimal' => 'xx']], 'b' => FILTER_VALIDATE_INT],
];
foreach ($specs as $spec) {
    A($data, $spec);
    A($data, $spec, false);
}
A(['a' => '1'], ['a' => ['filter' => FILTER_CALLBACK]]);
A(['a' => '1'], FILTER_CALLBACK);
A([], FILTER_CALLBACK);
A(['a' => [], 'b' => [[]]], FILTER_VALIDATE_INT);

// ------------------------------------------------------------ e-mail limits
foreach ([63, 64, 65] as $n) {
    foreach (['"' . str_repeat('a', $n - 1) . '\\x"', str_repeat('a', $n - 3) . '."a"', '"a".' . str_repeat('a', $n - 3), '"' . str_repeat('a', $n) . '"',
        '""' . str_repeat('a', $n), '"".' . str_repeat('a', $n - 1), str_repeat('a', $n), '"' . str_repeat('a', $n - 4) . '@xy"', str_repeat('a.', intdiv($n, 2)) . 'a'] as $lp) {
        V("$lp@b.cd", FILTER_VALIDATE_EMAIL);
        V("$lp@b.cd", FILTER_VALIDATE_EMAIL, FILTER_FLAG_EMAIL_UNICODE);
    }
}
$d = str_repeat('a', 63) . '.' . str_repeat('b', 63) . '.' . str_repeat('c', 63) . '.' . str_repeat('d', 61);
foreach (['a', '"a"', '"\\a"', 'ab', '"a".b', '"a\\"b"', 'ä'] as $lp) {
    for ($cut = 0; $cut <= 5; $cut++) {
        V("$lp@" . substr($d, $cut), FILTER_VALIDATE_EMAIL);
        V("$lp@" . substr($d, $cut), FILTER_VALIDATE_EMAIL, FILTER_FLAG_EMAIL_UNICODE);
    }
}
// quoted pairs with every byte
foreach ($bytes as $c) {
    V("\"a\\{$c}\"@b.cd", FILTER_VALIDATE_EMAIL);
    V("\"a\\{$c}\"@b.cd", FILTER_VALIDATE_EMAIL, FILTER_FLAG_EMAIL_UNICODE);
}
// many quoted words (PCRE backtracking in PHP's check)
for ($k = 1; $k <= 9; $k++) {
    foreach ([1, 6, 7, 8, 13, 14, 30, 52] as $m) {
        $atom = str_repeat('b', $m);
        foreach ([str_repeat('"a".', $k) . $atom, str_repeat('"a".', $k) . '"a"', str_repeat('"ab".', $k) . $atom, "$atom" . str_repeat('."a"', $k), '"a"' . str_repeat('.b', $k)] as $lp) {
            if (strlen($lp) > 70) continue;
            V("$lp@b.cd", FILTER_VALIDATE_EMAIL);
            V("$lp@b.cd", FILTER_VALIDATE_EMAIL, FILTER_FLAG_EMAIL_UNICODE);
        }
        $labels = [];
        $left = $m * 4;
        while ($left > 0) { $n = min(60, $left); $labels[] = str_repeat('c', $n); $left -= $n; }
        V(str_repeat('"a".', $k) . 'b@' . implode('.', $labels) . '.de', FILTER_VALIDATE_EMAIL);
    }
}
foreach (['"a""b"@b.cd', '"a"""b"@b.cd', '"a"@"b".cd', '"a\\@b"@b.cd', '"a".b."c"@b.cd', 'x."a".y."b".z@b.cd', '"a"."b".c.d.e@b.cd'] as $e) {
    V($e, FILTER_VALIDATE_EMAIL);
}
// letters and numbers beyond ASCII (FILTER_FLAG_EMAIL_UNICODE), as ranges
$prev = null;
$start = 0x80;
for ($cp = 0x80; $cp <= 0x30000; $cp++) {
    if ($cp >= 0xD800 && $cp < 0xE000) {
        $r = 'x';
    } elseif ($cp == 0x30000) {
        $r = 'end';
    } else {
        $ch = u8chr($cp);
        $r = (filter_var("a{$ch}b@b.cd", FILTER_VALIDATE_EMAIL, FILTER_FLAG_EMAIL_UNICODE) !== false ? '1' : '0')
            . (filter_var("\"a{$ch}b\"@b.cd", FILTER_VALIDATE_EMAIL, FILTER_FLAG_EMAIL_UNICODE) !== false ? '1' : '0');
    }
    if ($r !== $prev) {
        if ($prev !== null && $prev !== 'x') {
            printf("U %X %X %s\n", $start, $cp - 1, $prev);
        }
        $prev = $r;
        $start = $cp;
    }
}
foreach ([0xE0001, 0xE0100, 0xF0000, 0x10FFFD, 0x10FFFF, 0x31350] as $cp) {
    V('a' . u8chr($cp) . 'b@b.cd', FILTER_VALIDATE_EMAIL, FILTER_FLAG_EMAIL_UNICODE);
}

// ------------------------------------------------------------ more ip
foreach (['::1:1.2.3.4', '2003::1.2.3.4', 'fe80::1.2.3.4', 'fc00::1.2.3.4', '::ffff:1.2.3.4', '::ffff:192.168.0.1', '1:2:3:4:5:6:1.2.3.4', '::fffe:1.2.3.4',
    '2001:db8::1.2.3.4', '::0.0.0.2', '::0:1.2.3.4', '::1.2.3.4', '100::1.2.3.4', '2003:0:0:0:0:0:1.2.3.4', '::ffff:10.0.0.1', '::ffff:127.0.0.1'] as $ip) {
    foreach ([0, FILTER_FLAG_NO_PRIV_RANGE, FILTER_FLAG_NO_RES_RANGE, FILTER_FLAG_GLOBAL_RANGE, FILTER_FLAG_IPV4] as $fl) {
        V($ip, FILTER_VALIDATE_IP, $fl);
    }
}

// ------------------------------------------------------------ more float
foreach (['0e', '0.0e', '1e', '0.5e', '5e', '0.1e', '-0e'] as $p) {
    foreach ([-400, -325, -324, -323, -322, -1, 0, 1, 2, 307, 308, 309, 310, 400] as $n) {
        V($p . $n, FILTER_VALIDATE_FLOAT);
    }
}
foreach (['1e0000000000000000000000000001', '1e-0000000000000000000000000001', '1e99999999999999999999', '0e99999999999999999999', '1e-99999999999999999999', '2.4703282292062328e-324',
    '2.4703282292062327e-324', '1.7976931348623158e308', '1.79769313486231580793728971405301e308', str_repeat('9', 400), '0.' . str_repeat('0', 400) . '1', '0.' . str_repeat('0', 400),
    str_repeat('1', 400) . 'e-400', '1' . str_repeat('0', 400) . 'e-400', '0' . str_repeat('0', 500) . 'e400', '123456789e-330', '4.35e-321', '9223372036854775807', '9223372036854775808',
    '-9223372036854775808', '-9223372036854775809', '99999999999999999999', '-0', '-00', '+0', '-0.0', '-0e0', '1,000.e2', '-,100', "1,000'000", '0,000', '000,000', '0000,000', '1e1,000',
    '.5,000', '1.5,000', '1,234.5,6', '1e+-1', '1.5e3.'] as $s) {
    V($s, FILTER_VALIDATE_FLOAT);
    V($s, FILTER_VALIDATE_FLOAT, FILTER_FLAG_ALLOW_THOUSAND);
}

// ------------------------------------------------------------ option conversions
foreach ([['min_range' => 1e30], ['max_range' => NAN], ['max_range' => -1e30], ['min_range' => INF], ['min_range' => '1e1'], ['min_range' => '0x10'], ['min_range' => ' 5'],
    ['max_range' => 9.2233720368547758E+18], ['min_range' => -9.2233720368547758E+18]] as $o) {
    foreach (['1', '5', '-1', '16'] as $s) {
        V($s, FILTER_VALIDATE_INT, ['options' => $o]);
    }
}
foreach ([['uri_parser_class' => 'Nope'], ['uri_parser_class' => 123], ['options' => ['uri_parser_class' => 'Nope']], ['options' => ['uri_parser_class' => '']],
    ['options' => ['uri_parser_class' => 'uri\rfc3986\uri']], ['options' => ['uri_parser_class' => null]], ['options' => ['uri_parser_class' => 1]]] as $o) {
    V('http://a.b/', FILTER_VALIDATE_URL, $o);
    V('http://a_b/', FILTER_VALIDATE_URL, $o);
}
echo "END\n";
