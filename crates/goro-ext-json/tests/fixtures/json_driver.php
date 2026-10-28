<?php
// Fixture driver for goro-ext-json, run once on PHP 8.5:
//   php -d error_reporting=-1 -d display_errors=1 -d log_errors=0 \
//       -d html_errors=0 json_driver.php > json_php85.txt
// Every line is self-contained: inputs are written in the line (strings
// %-escaped, values as serialize() output), so the Rust test
// (tests/php85_fixture.rs) rebuilds them from the line itself. The
// X lines are named scenarios that the Rust test sets up by name.
//
// Line formats (fields separated by one space; ' starts an escaped string):
//   D a=<0|1|n> d=<depth> f=<flags> '<json> => '<serialize(result)> <err> v=<0|1> <verr>
//   E f=<flags> d=<depth> p=<serialize_precision> '<serialize(value)> => '<json>|!false <err>
//   X <name> => '<output>

ini_set('serialize_precision', -1);
mt_srand(20261028);

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

function ser($v): string {
    $p = ini_get('serialize_precision');
    ini_set('serialize_precision', -1);
    $s = serialize($v);
    ini_set('serialize_precision', $p);
    return $s;
}

$IG = JSON_INVALID_UTF8_IGNORE;
$SU = JSON_INVALID_UTF8_SUBSTITUTE;
$BI = JSON_BIGINT_AS_STRING;
$OA = JSON_OBJECT_AS_ARRAY;

function D(string $json, $assoc, int $depth, int $flags): void {
    $r = json_decode($json, $assoc, $depth, $flags);
    $e = json_last_error();
    $v = json_validate($json, $depth, $flags & JSON_INVALID_UTF8_IGNORE);
    $ve = json_last_error();
    $a = $assoc === null ? 'n' : (int)$assoc;
    echo "D a=$a d=$depth f=$flags ", esc($json), " => ", esc(ser($r)), " $e v=", (int)$v, " $ve\n";
}

function E($value, int $flags, int $depth = 512, int $prec = -1): void {
    ini_set('serialize_precision', $prec);
    $r = json_encode($value, $flags, $depth);
    $e = json_last_error();
    ini_set('serialize_precision', -1);
    echo "E f=$flags d=$depth p=$prec ", esc(ser($value)), " => ", $r === false ? '!false' : esc($r), " $e\n";
}

// ------------------------------------------------------------ decode: hand-picked
$hand = [
    '', ' ', '0', '-0', '-0.0', '0.0', '1', '-1', '123', '1.5', '-1.5e3', '1e5', '1E+5', '1e-5', '0e0', '-0e0',
    '0.0e-0', '1E400', '-1e400', '1.7976931348623157e308', '1.7976931348623159e308', '4.9e-324', '2e-324',
    '2.2250738585072011e-308', '2.2250738585072014e-308', '9007199254740993', '0.1', '0.30000000000000004',
    '123456789012345678', '1234567890123456789', '12345678901234567890', '9223372036854775807',
    '9223372036854775808', '-9223372036854775808', '-9223372036854775809', '18446744073709551616',
    '123456789012345678901234567890', '-123456789012345678901234567890', '123456789012345678901234567890.1',
    '[9223372036854775808, 1e30, -0, 0]', '01', '-01', '00', '1.', '.5', '-.5', '1e', '1e+', '1.e5', '+1', '--1',
    '-', '0x1', 'Infinity', '-Infinity', 'NaN', '1.5E', '1 2', '[1.]', '[.5]', '[1e]', '[01]', '[-]',
    'true', 'false', 'null', 'True', 'TRUE', 'nulL', 'Null', 'tru', 'nul', 'fals', 'falsey', 'truex', 'nullx',
    '[true]', '[True]', '[true false]', '[nul]', ' true', "\ttrue", "\ntrue", "\rtrue", "true ", "true\t",
    "true\n", "true\r", "\f[]", "\v[]", "[\f]", "\xc2\xa0[]", "\xef\xbb\xbf[1]", '""', '"abc"', '"', '""""',
    '"..".', '"\\"', '"\\\\"', '"\\/"', '"\\b\\f\\n\\r\\t"', '"\\x"', "\"\\'\"", '"\\017"', '"\\u"', '"\\u1"',
    '"\\u12"', '"\\u123"', '"\\u0041"', '"\\u004g"', '"\\U0041"', '"\\uZZZZ"', '"\\u00e9"', '"\\u00E9"',
    '"\\u20AC"', '"\\uFFFF"', '"\\uFFFE"', '"\\u0000"', '"a\\u0000b"', '"\\uD834\\uDD1E"', '"\\ud834\\udd1e"',
    '"\\uD834"', '"\\uDD1E"', '"\\uD834x"', '"\\uD834\\u0041"', '"\\uD834\\uD834"', '"\\uDC00\\uD800"',
    '"\\uD834\\uDD1E\\uD834"', '"\\uD834\\\\"', '"\\uD800\\uDBFF"', '"\\uD800\\u0000"', '"\\uD800\\"',
    '"\\uD800\\u"', '"\\uD800\\uDC0"', '"\\uD800\\uDC00"', '"\\uDBFF\\uDFFF"', '"\\uD800\\uE000"',
    '"\\uD800\\n"', '"\\uD800 \\uDC00"', '"\\uDFFF"', "\"\\ud834\x01\"", "\"\\ud800 \x01\"", '"\\uD800\\uZZZZ"',
    "\"a\x01\"", "\"\x00\"", "\"\x1f\"", "\"\x7f\"", "\"\t\"", "\"\n\"", "\"abc", "\"\\", "\"\\u", "\"a\\",
    "\"\\u12", "\"\xff\x01\"", "\"\x01\xff\"", "\x00", "\x01", "\x7f", "\xff", "[1]\x00", "[1]\xff", "[1] x",
    "руссиш", "\"руссиш\"", "[\x01]", " \x00 ", "[1,\x00]", "[\x7f]", "[\xff]", "[\"a\" \x01]", "[1 \xff]",
    "{\"a\" 1 \xff}", "[\"\x01\"", "[\"\xff", "[\"\xff\"", "[\"\x7f\"]", "-\x01", "1\x01", "1e\x01", "1.\xff",
    "t\x01", "tru\xff", "{\x01", "[\x01", "{\"a\":\x01", "[1 \"a\x01\"]", "[1] \"abc", "{\"a\" \"b\xff\"}",
    "[1 \"\\ud800\"]", "[1 \"\\x\"]", "{\"a\":1 \"b\x01\"}", "[1 [", "[1 {", "[\"a\":1]",
    '[]', '{}', '[1]', '[[1]]', '[[[1]]]', '[1,2,3]', '[1,]', '[,]', '[,1]', '[1,,2]', '{,}', '{"a"}',
    '{"a":}', '{1:1}', '{"a":1,}', '{"a":1 "b":2}', '{"a" :1}', '{"a": 1 , "b" : 2 }', '[]]', '{}}', '[}',
    '{]', '[1}', '{"a":1]', '[{]', '[{}}', '{"a":[}', '[1,}', '{"a"]', '{"a":]', '[1:', '{"a",', '[1 2]',
    '{[', '[1,2', '{"a":1', '{"a"', '{"a":', '{', '[', '"a', '[[]', '[{}', '{"a":{}', '{"a":[]',
    '{"a":1,"b":2,"a":3}', '{"a":1,"a":2}', '{"0":1,"1":2}', '{"1":1,"0":2}', '{"0":1,"a":2}', '{"":1}',
    '{"":1,"":2}', '{"_empty_":1}', '{"":{"":""}}', '{"-0":1}', '{"-1":1}', '{"01":1}', '{"1.5":1}',
    '{"9223372036854775807":1}', '{"9223372036854775808":1}', '{"-9223372036854775808":1}', '{" 1":1}',
    '{"\\u0000":1}', '{"\\u0000a":1}', '{"a\\u0000":1}', '{"\\u0000ab":1,"c":2}', '[{"key1":0,"\\u0000":1}]',
    '{"key":{"\\u0000":"aa"}}', "{\"\\u0000\":1\x01", '{"\\u0000":1 x', '{"\\u0000":1]', "{\"\\u0000\":1,\x01",
    '{"\\u0000":1,', '{"\\u0000":1', '{"\\u0000":1}x', "{\"\\u0000\":1}\x01", "{\"a\":{\"\\u0000\":1}\x01",
    "{\"\\u0000\":1,\"\\u0000\":2,\x01", '{"\\u0000":[[1]]}', "{\"\\u0000\":{\"\x01\"}}", "{\"\\u0000\xff\":1}",
    "{\"\xff\\u0000\":1}", "{\"\xff\":1}", "{\"a\xffb\":1}", "[1,\"\xff\"]", '  [  1  ,  2  ]  ',
    "[\n1,\n2\n]", '[[[[[[[[[[[[[[[[[[["Not too deep"]]]]]]]]]]]]]]]]]]]', '{"a":{"b":{"c":{"d":1}}}}',
    '[{"a":[{"b":[1]}]}]', '[[1],x]', "[1,[\"\x01\"]]", '[["\\ud800"]]', '["\\ud800",[[1]]]', "[[\x01",
    "[[\xff", '[[x', '{"a":[', '[{"a":[1]}]', '{"a":1,"b":[1,2],"c":{"d":42}}',
    '{"test":{} "foo":"bar"}', '{ "test": { "foo": "bar" } }', '{ "": "": "" } }', '{ "": { "": "" }',
    "['single quote']", '{unquoted_key: "x"}', '{"Illegal expression": 1 + 2}', '{"Numbers cannot be hex": 0x14}',
    '{"Double colon":: null}', '{"Comma instead of colon", null}', '["Colon instead of comma": false]',
    '["Bad value", truth]', '["Illegal backslash escape: \\x15"]',
    '{"myInt":99,"myFloat":123.45,"myNull":null,"myBool":true,"myString":"Hello World"}',
    '[0.5 ,98.6\n,\n99.44\n,\n\n1066\n\n\n,"rosebud"]', '{"E": 1.234567890E+34, "": 23456789012E666}',
    '{"zero": 0e0}', '[2.1]', '[123,13452345]', '"Hello World\\r\\n"', '"\\/\\\\\\"\\uCAFE\\uBABE\\uAB98\\uFCDE\\ubcda\\uef4A\\b\\f\\n\\r\\t`1~!@#$%^&*()_+-=[]{}|;:\',./<>?"',
];
$hand[] = str_repeat('[', 600) . str_repeat(']', 600);
$hand[] = str_repeat('{"a":', 40) . '1' . str_repeat('}', 40);
$hand[] = str_repeat('[', 513) . str_repeat(']', 513);
$hand[] = str_repeat('[', 512) . str_repeat(']', 512);
$hand[] = str_repeat('[', 511) . str_repeat(']', 511);
$opts = [[false, 512, 0], [true, 512, 0], [null, 512, $OA], [false, 1, 0], [false, 2, 0],
         [false, 512, $BI], [true, 512, $IG], [false, 512, $SU], [true, 512, $IG | $SU]];
foreach ($hand as $j) {
    foreach ($opts as [$a, $d, $f]) {
        D($j, $a, $d, $f);
    }
}

// ------------------------------------------------------------ UTF-8 sequences
$leads = [0x80, 0xbf, 0xc0, 0xc1, 0xc2, 0xdf, 0xe0, 0xe1, 0xed, 0xee, 0xef, 0xf0, 0xf1, 0xf4, 0xf5, 0xf8, 0xfe, 0xff];
$follow = [0x41, 0x80, 0x8f, 0x90, 0x9f, 0xa0, 0xbf, 0xc0, 0xc2, 0xe0, 0xf5, 0xff];
$seqs = [];
foreach ($leads as $l) {
    $seqs[] = chr($l);
    foreach ($follow as $f1) {
        $seqs[] = chr($l) . chr($f1);
        if ($l >= 0xe0 && $l <= 0xf4) {
            foreach ([0x41, 0x80, 0xc2, 0xc0] as $f2) {
                $seqs[] = chr($l) . chr($f1) . chr($f2);
                if ($l >= 0xf0) {
                    foreach ([0x41, 0x80, 0xc0] as $f3) {
                        $seqs[] = chr($l) . chr($f1) . chr($f2) . chr($f3);
                    }
                }
            }
        }
    }
}
foreach (["\xe2\x80\xa8", "\xe2\x80\xa9", "\xe2\x80\xa7", "\xe2\x80\xaa", "\xef\xbf\xbd", "\xf4\x8f\xbf\xbf",
          "\xed\x9f\xbf", "\xee\x80\x80", "\xc2\x80", "\xdf\xbf", "\xe0\xa0\x80", "\xf0\x90\x80\x80"] as $s) {
    $seqs[] = $s;
}
$uflags = [0, $IG, $SU, $IG | $SU, JSON_UNESCAPED_UNICODE, JSON_UNESCAPED_UNICODE | $SU, JSON_UNESCAPED_UNICODE | $IG,
           JSON_PARTIAL_OUTPUT_ON_ERROR, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_LINE_TERMINATORS,
           JSON_UNESCAPED_LINE_TERMINATORS];
$extra = count($seqs) - 12;
foreach ($seqs as $si => $s) {
    foreach (["a{$s}z", "a{$s}"] as $str) {
        foreach ($si >= $extra ? $uflags : [0, $IG, $SU, JSON_UNESCAPED_UNICODE | $SU] as $f) {
            E($str, $f);
        }
        foreach ([[false, 512, 0], [false, 512, $IG], [false, 512, $SU]] as [$a, $d, $f]) {
            D("\"$str\"", $a, $d, $f);
        }
    }
}

// ------------------------------------------------------------ random values
function rnd_str(): string {
    static $pieces = null;
    if ($pieces === null) {
        $pieces = ['a', 'b', 'Z', '0', '9', ' ', '"', '\\', '/', '<', '>', '&', "'", "\x00", "\x01", "\x08", "\t",
                   "\n", "\x0c", "\r", "\x1f", "\x7f", 'é', 'ж', '€', '日本', "\u{1F600}", "\u{10000}", "\u{2028}",
                   "\u{2029}", "\u{FFFD}", "\xff", "\x80", "\xc3", "\xe2\x82", "\xf0\x9f\x98", "\xed\xa0\x80",
                   "\xc0\xaf", '12', '-', '.', 'e', '+', 'abc', 'the quick brown fox jumps over the lazy dog'];
    }
    $n = mt_rand(0, 12);
    $s = '';
    for ($i = 0; $i < $n; $i++) {
        $s .= $pieces[mt_rand(0, count($pieces) - 1)];
    }
    return $s;
}

function rnd_num_str(): string {
    $forms = [
        fn() => (string)mt_rand(-1000, 1000),
        fn() => (string)mt_rand(),
        fn() => mt_rand(0, 9) . '.' . mt_rand(0, 99999),
        fn() => ' ' . mt_rand(0, 99),
        fn() => mt_rand(0, 99) . ' ',
        fn() => mt_rand(1, 9) . 'e' . mt_rand(-30, 30),
        fn() => '0x' . dechex(mt_rand(0, 255)),
        fn() => '.' . mt_rand(0, 99),
        fn() => mt_rand(0, 9) . '.',
        fn() => '1e' . mt_rand(300, 400),
        fn() => str_repeat('9', mt_rand(18, 25)),
        fn() => '-' . str_repeat('9', mt_rand(18, 25)),
        fn() => '+' . mt_rand(0, 9),
        fn() => '-0',
        fn() => '0' . mt_rand(0, 9),
        fn() => mt_rand(0, 9) . 'abc',
        fn() => "\n" . mt_rand(0, 9) . "\t",
        fn() => mt_rand(0, 9) . '.0',
    ];
    return $forms[mt_rand(0, count($forms) - 1)]();
}

function rnd_float(): float {
    switch (mt_rand(0, 7)) {
        case 0: return mt_rand() / mt_getrandmax() * 10 ** mt_rand(-10, 25);
        case 1: return (float)mt_rand(-100000, 100000);
        case 2: return mt_rand(-1000, 1000) / 8;
        case 3: return 10.0 ** mt_rand(-330, 310);
        case 4: return -mt_rand() / 7.0;
        case 5: return [0.1, 0.2, 0.3, 1/3, 2/3, 0.1 + 0.2, 1e15, 1e16, 1e17, 1e21, 1e22, 1e-4, 1e-5, 1e-6, 1e-7,
                        123456789.12345, 5e-324, 2.2250738585072014e-308, 1.7976931348623157e308, -0.0, 0.0,
                        9007199254740992.0, 9007199254740993.0, 4.35, 0.5, 1.5, 2.5, 100.1, 12.0][mt_rand(0, 28)];
        case 6: return unpack('e', pack('J', mt_rand() << 32 | mt_rand()))[1];
        default: return mt_rand(1, 999999) * 10.0 ** mt_rand(-20, 20);
    }
}

function rnd_value(int $depth) {
    $t = mt_rand(0, $depth > 3 ? 7 : 11);
    switch ($t) {
        case 0: return null;
        case 1: return (bool)mt_rand(0, 1);
        case 2: return [0, 1, -1, PHP_INT_MAX, PHP_INT_MIN, mt_rand(), -mt_rand(), 2147483647, 2147483648][mt_rand(0, 8)];
        case 3: return rnd_float();
        case 4: case 5: return rnd_str();
        case 6: return rnd_num_str();
        case 7: return [NAN, INF, -INF, 1.0, -0.0][mt_rand(0, 4)];
        case 8: case 9: // list or array with holes
            $n = mt_rand(0, 5);
            $a = [];
            for ($i = 0; $i < $n; $i++) {
                $a[] = rnd_value($depth + 1);
            }
            if ($n > 1 && mt_rand(0, 3) == 0) {
                unset($a[mt_rand(0, $n - 1)]);
            }
            if ($n > 1 && mt_rand(0, 5) == 0) {
                $a = array_reverse($a, true);
            }
            return $a;
        case 10: // map
            $n = mt_rand(0, 5);
            $a = [];
            for ($i = 0; $i < $n; $i++) {
                $k = mt_rand(0, 3) == 0 ? mt_rand(-5, 20) : rnd_str();
                $a[$k] = rnd_value($depth + 1);
            }
            return $a;
        default: // stdClass
            $n = mt_rand(0, 4);
            $o = new stdClass;
            for ($i = 0; $i < $n; $i++) {
                $k = mt_rand(0, 4) == 0 ? (string)mt_rand(0, 9) : rnd_str();
                if ($k !== '' && $k[0] === "\0") {
                    $k = 'x' . $k;
                }
                $o->{$k} = rnd_value($depth + 1);
            }
            return $o;
    }
}

$eflags = [JSON_HEX_TAG, JSON_HEX_AMP, JSON_HEX_APOS, JSON_HEX_QUOT, JSON_FORCE_OBJECT, JSON_NUMERIC_CHECK,
           JSON_UNESCAPED_SLASHES, JSON_PRETTY_PRINT, JSON_UNESCAPED_UNICODE, JSON_PARTIAL_OUTPUT_ON_ERROR,
           JSON_PRESERVE_ZERO_FRACTION, JSON_UNESCAPED_LINE_TERMINATORS, $IG, $SU];
function rnd_flags(array $eflags): int {
    $f = 0;
    $n = mt_rand(0, 4);
    for ($i = 0; $i < $n; $i++) {
        $f |= $eflags[mt_rand(0, count($eflags) - 1)];
    }
    return $f;
}

$docs = [];
for ($i = 0; $i < 2500; $i++) {
    $v = rnd_value(0);
    $f = rnd_flags($eflags);
    $d = mt_rand(0, 9) == 0 ? mt_rand(-1, 4) : 512;
    E($v, $f, $d);
    $j = json_encode($v, ($f | JSON_PARTIAL_OUTPUT_ON_ERROR) & ~JSON_PRETTY_PRINT | (mt_rand(0, 3) ? 0 : JSON_PRETTY_PRINT));
    if (is_string($j)) {
        $docs[] = $j;
    }
}

// Strings and numeric strings under every escaping flag.
for ($i = 0; $i < 1500; $i++) {
    $s = mt_rand(0, 4) ? rnd_str() : rnd_num_str();
    E($s, rnd_flags($eflags));
}

// Floats under several precisions.
for ($i = 0; $i < 1000; $i++) {
    $f = rnd_float();
    foreach ([-1, 17, 0, 5] as $p) {
        E($f, 0, 512, $p);
    }
    if ($i % 4 == 0) {
        foreach ([1, 2, 15, 16, 20, -1] as $p) {
            E($f, JSON_PRESERVE_ZERO_FRACTION, 512, $p);
        }
    }
}
foreach ([0.0, -0.0, 1.0, -1.0, 10.0, 0.5, 1.5, 2.5, 9.5, 0.05, 0.0001, 0.00001, 1e15, 1e16, 1e17, 123.0, 1e100,
          5e-324, 1e-300] as $f) {
    foreach ([-1, 0, 1, 2, 3, 5, 10, 15, 16, 17, 18, 25, 40] as $p) {
        E($f, 0, 512, $p);
        E($f, JSON_PRESERVE_ZERO_FRACTION, 512, $p);
    }
}
foreach ([" 12", "12 ", "1e5", "0x1A", "1e1000", "-1e1000", "99999999999999999999", ".5", "-0", "1.0", "1.", "+1",
          "  ", "", "1e-400", "0.1", "123343e871700", "007", "-.5e-3", "1_000", "١"] as $s) {
    foreach ([JSON_NUMERIC_CHECK, JSON_NUMERIC_CHECK | JSON_PRESERVE_ZERO_FRACTION] as $f) {
        E($s, $f);
        E([$s => $s], $f);
    }
}

// Depth and structure edge cases.
$nest = [];
for ($i = 0; $i < 6; $i++) {
    $nest = [$nest];
}
$on = new stdClass;
for ($i = 0; $i < 4; $i++) {
    $x = new stdClass;
    $x->o = $on;
    $on = $x;
}
foreach ([[], [[]], [1, [2, [3]]], $nest, new stdClass, $on, ['a' => ['b' => []]], [[1], [NAN]], [[[NAN]]],
          [[["\xff"]]], [NAN, [[1]]], [[1], "\xff"], ['k' => ["\xff" => 1]], [[["\xff" => 1]]], [1 => 1, 2 => 2],
          [3 => 'a'], [1 => 1, 0 => 0], [0 => 'a', 1 => 'b'], ['0' => 'a', '1' => 'b'], [-1 => 1], ["\0ab" => 1]] as $v) {
    foreach ([-1, 0, 1, 2, 3, 6, 7, 512, 2147483647, -2147483648] as $d) {
        foreach ([0, JSON_PARTIAL_OUTPUT_ON_ERROR, JSON_PRETTY_PRINT, JSON_FORCE_OBJECT | JSON_PRETTY_PRINT] as $f) {
            E($v, $f, $d);
        }
    }
}

// ------------------------------------------------------------ decode: generated
$numforms = [];
for ($i = 0; $i < 600; $i++) {
    $s = mt_rand(0, 3) ? '' : '-';
    $s .= mt_rand(0, 5) ? (string)mt_rand(1, 9) . substr(str_repeat((string)mt_rand(), 4), 0, mt_rand(0, 25)) : '0';
    if (mt_rand(0, 2) == 0) {
        $s .= '.' . substr(str_repeat((string)mt_rand(), 4), 0, mt_rand(1, 25));
    }
    if (mt_rand(0, 2) == 0) {
        $s .= ['e', 'E', 'e+', 'e-', 'E-'][mt_rand(0, 4)] . mt_rand(0, 400);
    }
    $numforms[] = $s;
}
foreach ($numforms as $s) {
    D($s, false, 512, 0);
    D("[$s]", true, 512, $BI);
}

$mut = ["\x00", "\x01", "\x1f", "\x7f", "\x80", "\xff", "\xc3", "\xe2\x82", '"', '\\', '[', ']', '{', '}', ',', ':',
        ' ', '0', '-', '.', 'e', 't', 'n', '\\u', '\\uD800', '\\uDC00', 'é', "\xc0\xaf"];
foreach (array_slice($docs, 0, 1200) as $n => $doc) {
    $variants = [$doc];
    for ($k = 0; $k < 2; $k++) {
        $s = $doc;
        $len = strlen($s);
        switch (mt_rand(0, 4)) {
            case 0: $s = substr($s, 0, mt_rand(0, $len)); break;
            case 1: $p = mt_rand(0, $len); $s = substr($s, 0, $p) . $mut[mt_rand(0, count($mut) - 1)] . substr($s, $p); break;
            case 2: if ($len) { $p = mt_rand(0, $len - 1); $s = substr($s, 0, $p) . substr($s, $p + 1); } break;
            case 3: if ($len) { $p = mt_rand(0, $len - 1); $s = substr($s, 0, $p) . $mut[mt_rand(0, count($mut) - 1)] . substr($s, $p + 1); } break;
            case 4: $s = $s . $mut[mt_rand(0, count($mut) - 1)]; break;
        }
        $variants[] = $s;
    }
    foreach ($variants as $s) {
        [$a, $d, $f] = $opts[mt_rand(0, count($opts) - 1)];
        if (mt_rand(0, 4) == 0) {
            $d = mt_rand(1, 6);
        }
        D($s, $a, $d, $f);
    }
}

// ------------------------------------------------------------ named scenarios
function X(string $name, callable $f): void {
    ob_start();
    try {
        $f();
    } catch (Throwable $e) {
        echo get_class($e), ': ', $e->getMessage(), ' (', $e->getCode(), ')';
    }
    $out = ob_get_clean();
    echo "X $name => ", esc($out), "\n";
}

class JS implements JsonSerializable {
    public function __construct(public $v, public $n = '') {}
    public function jsonSerialize(): mixed { echo "[js {$this->n}]"; return $this->v; }
}
class JSThrow implements JsonSerializable {
    public function jsonSerialize(): mixed { throw new Exception('Not implemented!'); }
}
class JSSelfEncode implements JsonSerializable {
    public $a = 1;
    private $b = 'hide';
    protected $c = 'protect';
    public function jsonSerialize(): mixed { echo ser(json_encode($this)), json_last_error(); return $this; }
}
class JSInnerEncode implements JsonSerializable {
    public $p;
    public function jsonSerialize(): mixed { echo ser(json_encode($this->p)), json_last_error(); return 1; }
}
class Vis { public $a = 1; protected $b = 2; private $c = 3; public $e; }
enum Unit { case A; }
enum BStr: string { case A = 'x'; case Q = 'a"b'; }
enum BInt: int { case A = 5; }
function show($r) { echo ser($r), ' ', json_last_error(), ' ', json_last_error_msg(); }

X('js_chain', fn() => show(json_encode(new JS(new JS(new JS(5, 'z'), 'y'), 'x'))));
X('js_array', fn() => show(json_encode(new JS([1, 'a' => new JS(2, 'in')], 'out'))));
X('js_nan', fn() => show(json_encode(new JS(NAN, 'n'), JSON_PARTIAL_OUTPUT_ON_ERROR)));
X('js_nan_then', fn() => show(json_encode([NAN, new JS(1, 'after')])));
X('js_utf8_stops', fn() => show(json_encode(["\xff", new JS(1, 'after')])));
X('js_utf8_partial', fn() => show(json_encode(["\xff", new JS(1, 'after')], JSON_PARTIAL_OUTPUT_ON_ERROR)));
X('js_badkey_goes_on', fn() => show(json_encode([["\xff" => 1], new JS(1, 'after')])));
X('js_depth_inner', fn() => show(json_encode([[new JS(1, 'in')]], 0, 1)));
X('js_depth_stops', fn() => show(json_encode([[1], new JS(1, 'in')], 0, 1)));
X('js_depth_partial', fn() => show(json_encode([[[1]], new JS(1, 'b'), NAN], JSON_PARTIAL_OUTPUT_ON_ERROR, 1)));
X('js_ret_depth', fn() => show(json_encode(new JS([1, [2]], 'd'), 0, 1)));
X('js_ret_depth2', fn() => show(json_encode(new JS([1, [2]], 'd'), 0, 2)));
X('js_scalar_depth0', fn() => show(json_encode(new JS(5, 'd'), 0, 0)));
X('js_self', function () { $o = new JS(null, 'self'); $o->v = $o; show(json_encode($o)); });
X('js_self_partial', function () { $o = new JS(null, 'self'); $o->v = $o; show(json_encode($o, JSON_PARTIAL_OUTPUT_ON_ERROR)); });
X('js_in_own_array', function () { $o = new JS(null, 'arr'); $o->v = [$o]; show(json_encode($o, JSON_PARTIAL_OUTPUT_ON_ERROR)); });
X('js_twice', function () { $o = new JS(1, 'tw'); show(json_encode([$o, $o, [$o]])); });
X('js_throw', fn() => show(json_encode([new JSThrow, new JSThrow])));
X('js_self_encode', fn() => show(json_encode(new JSSelfEncode)));
X('js_inner_plain_guard', function () { $p = new stdClass; $n = new JSInnerEncode; $n->p = $p; $p->n = $n; show(json_encode($p)); });
X('js_inner_array_guard', function () { $arr = ['x' => 1]; $n = new JSInnerEncode; $n->p = &$arr; $arr['n'] = $n; show(json_encode($arr)); });
X('js_pretty_nested', fn() => show(json_encode([new JS(json_encode([1], JSON_PRETTY_PRINT), 'p')])));
X('obj_recursion', function () { $a = new stdClass; $a->prop = $a; show(json_encode($a)); });
X('obj_recursion_partial', function () { $a = new stdClass; $a->prop = $a; show(json_encode($a, JSON_PARTIAL_OUTPUT_ON_ERROR)); });
X('obj_shared', function () { $q = new stdClass; $q->x = 1; show(json_encode([$q, $q, [$q]])); });
X('obj_in_array_cycle', function () { $p = new stdClass; $p->a = [$p]; show(json_encode($p, JSON_PARTIAL_OUTPUT_ON_ERROR)); });
X('arr_ref_recursion', function () { $a = [1]; $a[] = &$a; show(json_encode($a)); });
X('arr_ref_recursion_partial', function () { $a = [1]; $a[] = &$a; show(json_encode($a, JSON_PARTIAL_OUTPUT_ON_ERROR)); });
X('arr_ref_siblings', function () { $b = [1]; $c = [&$b, &$b]; show(json_encode($c)); });
X('arr_ref_elem', function () { $a = [1, 2]; $r = &$a[0]; show(json_encode($a)); });
X('visibility', fn() => show(json_encode(new Vis)));
X('enum_unit', fn() => show(json_encode(Unit::A)));
X('enum_unit_partial', fn() => show(json_encode([Unit::A, 1], JSON_PARTIAL_OUTPUT_ON_ERROR)));
X('enum_backed', fn() => show(json_encode([BStr::A, BInt::A, 'k' => BStr::Q], JSON_HEX_QUOT)));
X('resource', fn() => show(json_encode(fopen('php://memory', 'r'))));
X('resource_partial', fn() => show(json_encode(['a' => fopen('php://memory', 'r')], JSON_PARTIAL_OUTPUT_ON_ERROR)));
X('throw_encode', fn() => show(json_encode("\x80", JSON_THROW_ON_ERROR)));
X('throw_partial_encode', fn() => show(json_encode("\x80", JSON_THROW_ON_ERROR | JSON_PARTIAL_OUTPUT_ON_ERROR)));
X('throw_decode', fn() => show(json_decode('{', false, 512, JSON_THROW_ON_ERROR)));
X('throw_decode_empty', fn() => show(json_decode('', false, 0, JSON_THROW_ON_ERROR)));
X('state_throw_ok', function () { json_decode("\xff"); show(json_encode(1, JSON_THROW_ON_ERROR)); });
X('state_throw_decode_ok', function () { json_decode("\xff"); show(json_decode('[1]', false, 512, JSON_THROW_ON_ERROR)); });
X('state_throw_fail', function () { json_decode("\xff"); try { json_encode(NAN, JSON_THROW_ON_ERROR); } catch (JsonException $e) { echo $e->getMessage(), $e->getCode(), ' '; } show(1); });
X('state_throw_decode_fail', function () { json_decode("\xff"); try { json_decode('', false, 512, JSON_THROW_ON_ERROR); } catch (JsonException $e) { echo $e->getMessage(), $e->getCode(), ' '; } show(1); });
X('state_decode_valueerror', function () { json_decode("\xff"); try { json_decode('1', false, 0); } catch (ValueError $e) { echo $e->getMessage(), ' '; } show(1); });
X('state_decode_valueerror_throw', function () { json_decode("\xff"); try { json_decode('1', false, 0, JSON_THROW_ON_ERROR); } catch (ValueError $e) { echo $e->getMessage(), ' '; } show(1); });
X('state_validate_valueerror', function () { json_decode("\xff"); try { json_validate('1', 0); } catch (ValueError $e) { echo $e->getMessage(), ' '; } show(1); });
X('state_encode_reset', function () { json_decode("\xff"); show(json_encode(1, 0, -5)); });
X('decode_depth_big', fn() => show(json_decode('[1]', false, 2147483648)));
X('decode_depth_max', fn() => show(json_decode('[1]', false, 2147483647)));
X('decode_depth_neg', fn() => show(json_decode('[1]', false, -1)));
X('decode_empty_depth0', fn() => show(json_decode('', false, 0)));
X('decode_flags_unknown', fn() => show(json_decode('[1]', true, 512, -1)));
X('validate_empty_depth0', fn() => show(json_validate('', 0)));
X('validate_depth_big', fn() => show(json_validate('1', 2147483648)));
X('validate_flags', fn() => show(json_validate('1', 512, JSON_BIGINT_AS_STRING)));
X('validate_flags_depth0', fn() => show(json_validate('1', 0, JSON_BIGINT_AS_STRING)));
X('validate_empty_badflags', fn() => show(json_validate('', 512, 2)));
X('validate_ignore', fn() => show(json_validate("[\"\xff\"]", 512, JSON_INVALID_UTF8_IGNORE)));
X('encode_depth_intmax', fn() => show(json_encode([1], 0, PHP_INT_MAX)));
X('encode_depth_2p31', fn() => show(json_encode([1], 0, 2147483648)));
X('encode_depth_2p32p1', fn() => show(json_encode([1], 0, 4294967297)));
X('last_error_msgs', function () { foreach (["[1", "[}", "\"\x01\"", "\xff", "\"\\ud800\"", '{"\u0000":1}', '[[1]]'] as $j) { json_decode($j, false, 2); echo json_last_error(), ':', json_last_error_msg(), ';'; } });
X('decode_object_numeric_keys', fn() => show(json_decode('{"0":1,"1":2,"a":{"2":3}}')));
X('decode_object_dup', fn() => show(json_decode('{"a":1,"b":2,"a":3}')));
X('encode_decoded', fn() => show(json_encode(json_decode('{"0":1,"":2,"a":{"b":[]},"c":{}}'))));
