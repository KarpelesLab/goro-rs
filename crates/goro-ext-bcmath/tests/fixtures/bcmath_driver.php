<?php
// Generates crates/goro-ext-bcmath/tests/fixtures/bcmath_php85.txt.
// Run once on the reference PHP 8.5 binary:
//   php -d error_reporting=-1 -d display_errors=1 -d log_errors=0 -d html_errors=0 \
//       -d memory_limit=1G bcmath_driver.php > bcmath_php85.txt
//
// Line format (tab separated): op, args..., "=>", result.
// Args and results escape bytes outside 0x21..0x7e (and the backslash) as \xHH.
// A null argument is "~". Typed operands: "s:<str>", "i:<int>", "n:<Number ctor str>".
// "@seed,int,frac,neg" stands for a large operand built by gen() below.
// Result: "=<string>", "N(<value>,<scale>)", "[<q>|<r>]", "!<Class>: <message>",
// or "#<len>:<crc32>" for results longer than 300 bytes, after any
// "{D:<message>}" / "{W:<message>}" notices raised during the call.
ini_set('bcmath.scale', '0');
mt_srand(20261028);

function enc($s) {
    if ($s === null) return '~';
    return preg_replace_callback('/[^\x21-\x5b\x5d-\x7e]/', fn($m) => sprintf('\\x%02x', ord($m[0])), (string) $s);
}
function res($v) {
    if ($v instanceof BcMath\Number) {
        $s = "N(" . $v->value . "," . $v->scale . ")";
    } elseif (is_array($v)) {
        $s = "[" . res($v[0]) . "|" . res($v[1]) . "]";
        return $s;
    } elseif (is_int($v)) {
        $s = "=" . $v;
    } else {
        $s = "=" . $v;
    }
    if (strlen($s) > 300) return "#" . strlen($s) . ":" . sprintf('%08x', crc32($s));
    return enc($s);
}
$NOTES = [];
set_error_handler(function ($no, $msg) {
    global $NOTES;
    $NOTES[] = ($no === E_DEPRECATED ? 'D' : ($no === E_WARNING ? 'W' : 'E' . $no)) . ':' . $msg;
    return true;
});
function line($op, array $args, callable $f) {
    global $NOTES;
    $NOTES = [];
    try {
        $r = res($f());
    } catch (Throwable $e) {
        $r = "!" . get_class($e) . ": " . enc($e->getMessage());
    }
    foreach ($NOTES as $n) $r = '{' . enc($n) . '}' . $r;
    echo $op, "\t", implode("\t", array_map('enc', $args)), "\t=>\t", $r, "\n";
}
function typed($t) {
    // "s:..", "i:..", "n:.."
    $k = substr($t, 0, 2);
    $v = substr($t, 2);
    if ($k === 'i:') return (int) $v;
    if ($k === 'n:') return new BcMath\Number($v);
    return $v;
}

function rdigits($n) {
    $s = '';
    for ($i = 0; $i < $n; $i++) $s .= chr(48 + mt_rand(0, 9));
    return $s;
}
function rnum($maxInt = 22, $maxFrac = 22) {
    $r = mt_rand(0, 99);
    if ($r < 6) {
        $specials = ['0', '-0', '', '.', '-0.000', '0.00', '1', '-1', '+5', '.5', '-.25', '7.', '0001.1000', '-0', '+0.0', '10', '-10.0'];
        return $specials[mt_rand(0, count($specials) - 1)];
    }
    $il = mt_rand(0, 3) == 0 ? mt_rand(0, 3) : mt_rand(0, $maxInt);
    $fl = mt_rand(0, 3) == 0 ? 0 : mt_rand(0, $maxFrac);
    $int = rdigits($il);
    if ($il > 0 && mt_rand(0, 4) == 0) $int = str_repeat('0', mt_rand(1, 3)) . $int;
    $frac = rdigits($fl);
    if ($fl > 0 && mt_rand(0, 4) == 0) $frac .= str_repeat('0', mt_rand(1, 4));
    if ($fl > 0 && mt_rand(0, 6) == 0) $frac = str_repeat('0', mt_rand(1, 6)) . $frac;
    $s = $int;
    if ($fl > 0 || mt_rand(0, 9) == 0) $s .= '.' . $frac;
    if ($s === '') $s = '0';
    $sg = mt_rand(0, 9);
    if ($sg < 4) $s = '-' . $s;
    elseif ($sg == 4) $s = '+' . $s;
    return $s;
}
function rscale() {
    $r = mt_rand(0, 19);
    if ($r == 0) return null;
    if ($r == 1) return mt_rand(20, 60);
    return mt_rand(0, 12);
}
// Two numbers of similar shape, to exercise cancellation and near-equal comparisons.
function rnear($a) {
    if (mt_rand(0, 2) > 0) return rnum();
    $b = $a;
    $len = strlen($b);
    if ($len > 0) {
        $p = mt_rand(0, $len - 1);
        if (ctype_digit($b[$p])) $b[$p] = chr(48 + mt_rand(0, 9));
    }
    if (mt_rand(0, 1)) $b = ($b !== '' && $b[0] === '-') ? substr($b, 1) : '-' . $b;
    return $b;
}

$bad = [' 1', '1 ', "\t1", "1\n", '1e5', '0x1A', '--1', '+-1', '1..2', '1.2.3', "\u{661}", '12a', 'a', '1_000', '++1', '-+1', '.-1', '1.-1', '1,5', '- 1', "1\0abc", "\0", "-\x001", '١٢', 'INF', 'NAN', '1.5e3', '.e1', '0b1', '٫5'];

// ---------- default scale ----------
foreach ([0, 3, 0] as $sc) {
    echo "scale\t$sc\t=>\t=" . bcscale($sc) . "\n";
    foreach (['1.23456', '-7.5', '0.0005'] as $a) {
        line('add', [$a, '1.1', null], fn() => bcadd($a, '1.1'));
        line('sub', [$a, '1.1', null], fn() => bcsub($a, '1.1'));
        line('mul', [$a, '1.1', null], fn() => bcmul($a, '1.1'));
        line('div', [$a, '1.1', null], fn() => bcdiv($a, '1.1'));
        line('mod', [$a, '1.1', null], fn() => bcmod($a, '1.1'));
        line('divmod', [$a, '1.1', null], fn() => bcdivmod($a, '1.1'));
        line('pow', [$a, '3', null], fn() => bcpow($a, '3'));
        line('sqrt', [ltrim($a, '-'), null], fn() => bcsqrt(ltrim($a, '-')));
        line('comp', [$a, '1.23', null], fn() => bccomp($a, '1.23'));
        line('powmod', ['7', '3', '5', null], fn() => bcpowmod('7', '3', '5'));
    }
}
bcscale(0);

// ---------- random binary ops ----------
$bin = [
    'add' => fn($a, $b, $s) => bcadd($a, $b, $s),
    'sub' => fn($a, $b, $s) => bcsub($a, $b, $s),
    'mul' => fn($a, $b, $s) => bcmul($a, $b, $s),
    'div' => fn($a, $b, $s) => bcdiv($a, $b, $s),
    'mod' => fn($a, $b, $s) => bcmod($a, $b, $s),
    'divmod' => fn($a, $b, $s) => bcdivmod($a, $b, $s),
    'comp' => fn($a, $b, $s) => bccomp($a, $b, $s),
];
foreach ($bin as $op => $f) {
    for ($i = 0; $i < 900; $i++) {
        $a = rnum();
        $b = rnear($a);
        if (($op === 'div' || $op === 'mod' || $op === 'divmod') && mt_rand(0, 30) == 0) $b = ['0', '-0.00', '', '.'][mt_rand(0, 3)];
        $s = rscale();
        line($op, [$a, $b, $s], fn() => $f($a, $b, $s));
    }
    // grid of small values and scales
    $small = ['0', '-0', '1', '-1', '0.5', '-0.5', '2', '-3', '0.001', '-0.001', '99.99', '-99.99', '0.1', '7', '1.000'];
    foreach ($small as $a) foreach ($small as $b) foreach ([0, 2] as $s) {
        line($op, [$a, $b, $s], fn() => $f($a, $b, $s));
    }
    // malformed inputs in each position, and bad scales
    foreach ($bad as $x) {
        line($op, [$x, '1', 2], fn() => $f($x, '1', 2));
        line($op, ['1', $x, 2], fn() => $f('1', $x, 2));
    }
    foreach ([-1, PHP_INT_MIN, 2147483648, PHP_INT_MAX] as $s) {
        line($op, ['a', '0', $s], fn() => $f('a', '0', $s));
        line($op, ['1', '1', $s], fn() => $f('1', '1', $s));
    }
    line($op, ['1', '1', 2147483647 - 2147483647], fn() => $f('1', '1', 0));
}
// division edge cases: long quotients, exact quotients, huge scale
foreach ([['1', '3', 50], ['2', '7', 60], ['-1', '7', 0], ['10', '3.3333', 20], ['123456789012345678901234567890', '0.000000000000000000001', 3],
          ['0.000000000000000000001', '123456789012345678901234567890', 60], ['1', '1' . str_repeat('0', 80), 90], ['-0.01', '-123456789000', 9],
          ['99999999999999999999', '99999999999999999999.0000000001', 25], ['1', '0.' . str_repeat('0', 40) . '1', 0]] as [$a, $b, $s]) {
    line('div', [$a, $b, $s], fn() => bcdiv($a, $b, $s));
    line('mod', [$a, $b, $s], fn() => bcmod($a, $b, $s));
    line('divmod', [$a, $b, $s], fn() => bcdivmod($a, $b, $s));
}

// ---------- pow ----------
for ($i = 0; $i < 1500; $i++) {
    $a = mt_rand(0, 3) ? rnum(4, 6) : rnum(12, 12);
    $e = (string) mt_rand(-12, 30);
    if (mt_rand(0, 10) == 0) $e .= '.0';
    if (mt_rand(0, 20) == 0) $e = '-0';
    $s = rscale();
    line('pow', [$a, $e, $s], fn() => bcpow($a, $e, $s));
}
foreach (['0', '-0', '0.00', '1', '-1', '1.000', '-1.0', '2', '-2', '0.5', '10'] as $a) {
    foreach (['0', '1', '-1', '2', '-2', '63', '-63', '64', '1128321638', '-1128321638', '9223372036854775806', '-9223372036854775807', '9223372036854775808', '-9223372036854775808', '99999999999999999999', '1.5', '-0.5', '2.0', '-2.000', '0.0'] as $e) {
        foreach ([0, 3] as $s) {
            // Huge exponents of |base| > 1 or fractional bases cannot finish; keep only the cheap ones.
            $big = strlen(ltrim($e, '-')) > 3 && !str_contains($e, '.');
            if ($big && !in_array($a, ['0', '-0', '0.00'], true) && strlen(ltrim($e, '-')) < 19) continue;
            if ($big && in_array($a, ['1', '-1', '1.000', '-1.0'], true)) continue;
            if ($big && in_array($a, ['2', '-2', '0.5', '10'], true) && strlen(ltrim($e, '-')) == 19) continue;
            line('pow', [$a, $e, $s], fn() => bcpow($a, $e, $s));
        }
    }
}
foreach ($bad as $x) {
    line('pow', [$x, '2', 1], fn() => bcpow($x, '2', 1));
    line('pow', ['2', $x, 1], fn() => bcpow('2', $x, 1));
}
foreach ([[ 'a', '1.5', -1], ['1', '1.5', -1], ['a', '1.5', 0], ['0', '-1.5', 0], ['0', '-1', 0], ['0', '-99999999999999999999', 0], ['0.0', '-0.0', 2]] as [$a, $e, $s]) {
    line('pow', [$a, $e, $s], fn() => bcpow($a, $e, $s));
}

// ---------- sqrt ----------
for ($i = 0; $i < 900; $i++) {
    $a = rnum();
    if (mt_rand(0, 4)) $a = ltrim($a, '-');
    $s = rscale();
    line('sqrt', [$a, $s], fn() => bcsqrt($a, $s));
}
foreach (['0', '-0', '1', '4', '2', '0.25', '0.0001', '0.00001', '100', '1000000', '0.5', '99'] as $a) foreach ([0, 1, 5, 30] as $s) {
    line('sqrt', [$a, $s], fn() => bcsqrt($a, $s));
}
foreach ($bad as $x) line('sqrt', [$x, 2], fn() => bcsqrt($x, 2));
foreach ([['a', -1], ['-1', -1], ['-1', 0], ['-0.00001', 2]] as [$a, $s]) line('sqrt', [$a, $s], fn() => bcsqrt($a, $s));

// ---------- powmod ----------
for ($i = 0; $i < 900; $i++) {
    $a = (mt_rand(0, 3) ? '' : '-') . rdigits(mt_rand(1, 25));
    $e = rdigits(mt_rand(1, 22));
    $m = (mt_rand(0, 5) ? '' : '-') . rdigits(mt_rand(1, 25));
    if (mt_rand(0, 9) == 0) $a .= '.000';
    $s = mt_rand(0, 4) ? 0 : rscale();
    line('powmod', [$a, $e, $m, $s], fn() => bcpowmod($a, $e, $m, $s));
}
$pmv = ['a', '1.5', '-1', '0', '1', '2', '-2', '1.0', '-0', '3', ''];
foreach ($pmv as $a) foreach ($pmv as $e) foreach ($pmv as $m) {
    line('powmod', [$a, $e, $m, 0], fn() => bcpowmod($a, $e, $m, 0));
}
foreach ([-1, 2147483648] as $s) line('powmod', ['a', 'b', 'c', $s], fn() => bcpowmod('a', 'b', 'c', $s));
foreach ($bad as $x) {
    line('powmod', [$x, '2', '5', 0], fn() => bcpowmod($x, '2', '5', 0));
    line('powmod', ['2', $x, '5', 0], fn() => bcpowmod('2', $x, '5', 0));
    line('powmod', ['2', '2', $x, 0], fn() => bcpowmod('2', '2', $x, 0));
}

// ---------- round / floor / ceil ----------
$modes = RoundingMode::cases();
for ($i = 0; $i < 2500; $i++) {
    $a = rnum();
    if (mt_rand(0, 3) == 0) {
        // values sitting on a rounding boundary
        $a = (mt_rand(0, 1) ? '-' : '') . rdigits(mt_rand(0, 6)) . '.' . rdigits(mt_rand(0, 4)) . '5' . (mt_rand(0, 1) ? str_repeat('0', mt_rand(1, 3)) : '');
    }
    $p = mt_rand(-8, 14);
    $m = $modes[mt_rand(0, count($modes) - 1)];
    line('round', [$a, $p, $m->name], fn() => bcround($a, $p, $m));
}
foreach (['0.5', '-0.5', '1.5', '2.5', '-2.5', '0.05', '-0.0', '9.5', '99.95', '-99.95', '0.4999999', '0.5000001', '5', '-5', '15', '25', '-25', '0', '', '.', '-', '999999.9999'] as $a) {
    foreach ([-3, -1, 0, 1, 3] as $p) foreach ($modes as $m) {
        line('round', [$a, $p, $m->name], fn() => bcround($a, $p, $m));
    }
}
foreach ([PHP_INT_MIN, -100, -20, 100] as $p) foreach ($modes as $m) {
    if ($p === PHP_INT_MIN && ($m === RoundingMode::AwayFromZero || $m === RoundingMode::PositiveInfinity)) continue;
    line('round', ['12.5', $p, $m->name], fn() => bcround('12.5', $p, $m));
}
line('round', ['1.5', 0, '~'], fn() => bcround('1.5'));
line('round', ['1.55', 1, '~'], fn() => bcround('1.55', 1));
foreach ($bad as $x) line('round', [$x, 0, 'HalfAwayFromZero'], fn() => bcround($x));
for ($i = 0; $i < 400; $i++) {
    $a = rnum();
    line('floor', [$a], fn() => bcfloor($a));
    line('ceil', [$a], fn() => bcceil($a));
}
foreach ($bad as $x) {
    line('floor', [$x], fn() => bcfloor($x));
    line('ceil', [$x], fn() => bcceil($x));
}

// ---------- bcscale ----------
foreach ([0, 5, -1, 2147483648, PHP_INT_MIN, 2147483647, 0] as $s) {
    line('bcscale', [$s], fn() => bcscale($s));
}
bcscale(0);

// ---------- BcMath\Number ----------
$ctor = ['s:0', 's:0.00', 's:.1', 's:-.1', 's:+.1', 's:0.20', 's:-0.000', 's:1234.0', 's:0001234.5600', 's:', 's:-', 's:+', 's:.', 's:1.',
         'i:0', 'i:-0', 'i:123', 'i:-9223372036854775807', 'i:' . PHP_INT_MIN, 'i:' . PHP_INT_MAX];
foreach (array_merge($ctor, array_map(fn($x) => 's:' . $x, $bad)) as $c) {
    line('ctor', [$c], fn() => new BcMath\Number(typed($c)));
}
foreach (['0', '-0', '0.0', '.5', '-', '007.50', '', 'a', ' 1', '1e3', "1\x00"] as $v) {
    $ser = 'O:13:"BcMath\Number":1:{s:5:"value";s:' . strlen($v) . ':"' . $v . '";}';
    line('unser', [$v], fn() => unserialize($ser));
}
line('unser', ['~'], fn() => unserialize('O:13:"BcMath\Number":0:{}'));
line('unser', ['i:5'], fn() => unserialize('O:13:"BcMath\Number":1:{s:5:"value";i:5;}'));

$methods = ['add', 'sub', 'mul', 'div', 'mod', 'divmod', 'pow', 'compare'];
function rarg() {
    $r = mt_rand(0, 9);
    if ($r < 2) return 'i:' . mt_rand(-1000, 1000);
    if ($r < 4) return 'n:' . rnum(8, 8);
    return 's:' . rnum(8, 8);
}
function rexp() {
    $r = mt_rand(0, 5);
    $e = mt_rand(-6, 9);
    if ($r == 0) return 'i:' . $e;
    if ($r == 1) return 'n:' . $e;
    if ($r == 2) return 's:' . $e . '.00';
    return 's:' . $e;
}
for ($i = 0; $i < 3000; $i++) {
    $m = $methods[mt_rand(0, count($methods) - 1)];
    $a = rnum(8, 8);
    if ($m === 'pow') { $a = rnum(3, 3); $b = rexp(); } else $b = rarg();
    $s = mt_rand(0, 3) ? null : mt_rand(0, 15);
    line('N.' . $m, [$a, $b, $s], fn() => (new BcMath\Number($a))->$m(typed($b), $s));
}
foreach (['0', '0.00', '4', '4.00', '2', '0.25', '15151324141414.412312232141241', '0.000001', '-1', '-0.00', '100', '0.1', '1.21'] as $a) {
    foreach ([null, 0, 3, 12] as $s) line('N.sqrt', [$a, $s], fn() => (new BcMath\Number($a))->sqrt($s));
}
foreach (['1.5', '-2.5', '0', '12.345', '-0.005', '99.5'] as $a) {
    foreach ([-2, 0, 1, 2, 5] as $p) foreach ($modes as $mo) {
        line('N.round', [$a, $p, $mo->name], fn() => (new BcMath\Number($a))->round($p, $mo));
    }
    line('N.floor', [$a], fn() => (new BcMath\Number($a))->floor());
    line('N.ceil', [$a], fn() => (new BcMath\Number($a))->ceil());
}
$pmn = ['s:a', 's:1.5', 's:-1', 's:0', 's:1', 's:3', 'i:-2', 'i:5', 'n:1.0', 'n:2.50', 's:-0'];
foreach (['7', '-7', '7.0', '7.5', '0'] as $a) foreach ($pmn as $e) foreach ($pmn as $md) {
    line('N.powmod', [$a, $e, $md, null], fn() => (new BcMath\Number($a))->powmod(typed($e), typed($md)));
}
line('N.powmod', ['7', 'i:3', 'i:5', 4], fn() => (new BcMath\Number('7'))->powmod(3, 5, 4));
line('N.powmod', ['7', 'i:3', 'i:5', -1], fn() => (new BcMath\Number('7'))->powmod(3, 5, -1));
line('N.powmod', ['7', 's:a', 'i:5', -1], fn() => (new BcMath\Number('7'))->powmod('a', 5, -1));
foreach (['add', 'sub', 'mul', 'div', 'mod', 'divmod', 'pow', 'compare'] as $m) {
    foreach (['s:a', 's: 1', 's:1e2', 's:0', 'n:0.000', 'i:0'] as $b) {
        foreach ([null, -1, 2147483648] as $s) {
            line('N.' . $m, ['5', $b, $s], fn() => (new BcMath\Number('5'))->$m(typed($b), $s));
        }
    }
    line('N.' . $m, ['5', 's:9223372036854775808', null], fn() => (new BcMath\Number('5'))->$m('9223372036854775808'));
    line('N.' . $m, ['5', 's:1.5', null], fn() => (new BcMath\Number('5'))->$m('1.5'));
    line('N.' . $m, ['0', 's:-1', null], fn() => (new BcMath\Number('0'))->$m('-1'));
}
foreach ([null, -1, 3] as $s) line('N.sqrt', ['-4', $s], fn() => (new BcMath\Number('-4'))->sqrt($s));

// operators
$opv = ['i:100', 'i:-20', 'i:3', 'i:0', 's:0.01', 's:-0.40', 's:7', 's:a', 's: 1', 's:', 'n:100', 'n:-0.40', 'n:3', 'n:0', 'n:2.50', 'i:-2', 's:-2', 'n:-2', 's:0.5', 'n:1.5', 'i:' . PHP_INT_MAX];
$ops = ['+', '-', '*', '/', '%', '**'];
foreach ($opv as $l) foreach ($opv as $r) {
    if (!str_starts_with($l, 'n:') && !str_starts_with($r, 'n:')) continue;
    foreach ($ops as $o) {
        if ($o === "**" && $r === "i:" . PHP_INT_MAX) continue;
        line('op' . $o, [$l, $r], function () use ($l, $r, $o) {
            $x = typed($l);
            $y = typed($r);
            return match ($o) {
                '+' => $x + $y, '-' => $x - $y, '*' => $x * $y, '/' => $x / $y, '%' => $x % $y, '**' => $x ** $y,
            };
        });
    }
    line('cmp', [$l, $r], function () use ($l, $r) {
        $x = typed($l);
        $y = typed($r);
        return ($x <=> $y) . ',' . ($x == $y ? 'T' : 'F') . ',' . ($x < $y ? 'T' : 'F') . ',' . ($x > $y ? 'T' : 'F');
    });
}
foreach (['0', '-0.00', '1.5', '-2'] as $a) {
    line('bool', [$a], fn() => (bool) new BcMath\Number($a) ? 'T' : 'F');
    line('neg', [$a], fn() => -(new BcMath\Number($a)));
    line('inc', [$a], function () use ($a) { $n = new BcMath\Number($a); $n++; return $n; });
    line('dec', [$a], function () use ($a) { $n = new BcMath\Number($a); $n--; return $n; });
}

// ---------- large operands (results hashed) ----------
// Operands come from a small LCG so that the fixture stores only the token
// "@seed,intdigits,fracdigits,neg" and the test rebuilds the number.
function gen($seed, $il, $fl, $neg) {
    $x = $seed;
    $s = $neg ? '-' : '';
    for ($i = 0; $i < $il + $fl; $i++) {
        $x = ($x * 1103515245 + 12345) % 2147483648;
        $d = ($x >> 16) % 10;
        if ($i == 0 && $d == 0) $d = 1;
        if ($i == $il) $s .= '.';
        $s .= chr(48 + $d);
    }
    return $s;
}
$seed = 1;
foreach ([[40, 5], [100, 0], [300, 120], [450, 450], [1000, 10], [2500, 700], [6000, 0]] as [$il, $fl]) {
    for ($k = 0; $k < 3; $k++) {
        $ta = [$seed++, $il, $fl, mt_rand(0, 2) ? 0 : 1];
        $tb = [$seed++, mt_rand(1, $il), mt_rand(0, $fl), mt_rand(0, 2) ? 0 : 1];
        $tp = [$ta[0], $il, $fl, 0];
        $a = gen(...$ta);
        $b = gen(...$tb);
        $p = gen(...$tp);
        $A = '@' . implode(',', $ta);
        $B = '@' . implode(',', $tb);
        $P = '@' . implode(',', $tp);
        $s = [0, 7, $fl + 3][$k];
        line('add', [$A, $B, $s], fn() => bcadd($a, $b, $s));
        line('sub', [$A, $B, $s], fn() => bcsub($a, $b, $s));
        line('mul', [$A, $B, $s], fn() => bcmul($a, $b, $s));
        line('mul', [$A, $A, $s], fn() => bcmul($a, $a, $s));
        line('div', [$A, $B, $s], fn() => bcdiv($a, $b, $s));
        line('div', [$B, $A, $s + 50], fn() => bcdiv($b, $a, $s + 50));
        line('div', [$A, '7', $s], fn() => bcdiv($a, '7', $s));
        line('div', [$A, '1234567890123456789', $s], fn() => bcdiv($a, '1234567890123456789', $s));
        line('mod', [$A, $B, $s], fn() => bcmod($a, $b, $s));
        line('divmod', [$A, $B, $s], fn() => bcdivmod($a, $b, $s));
        line('comp', [$A, $B, $s], fn() => bccomp($a, $b, $s));
        line('sqrt', [$P, $s], fn() => bcsqrt($p, $s));
        line('sqrt', [$P, $il], fn() => bcsqrt($p, $il));
        line('round', [$A, intdiv($fl, 2), 'HalfEven'], fn() => bcround($a, intdiv($fl, 2), RoundingMode::HalfEven));
        line('N.div', [$A, 's:' . $B, null], fn() => (new BcMath\Number($a))->div($b));
        line('N.sqrt', [$P, null], fn() => (new BcMath\Number($p))->sqrt());
    }
}
foreach ([['3', '2000', 0], ['1.0001', '3000', 20], ['-12345678901234567890.5', '41', 3], ['7', '-300', 400], ['1.5', '-77', 30], ['123456789', '777', 0], ['0.999', '1000', 10]] as [$a, $e, $s]) {
    line('pow', [$a, $e, $s], fn() => bcpow($a, $e, $s));
}
for ($k = 0; $k < 6; $k++) {
    $ta = [$seed++, mt_rand(50, 300), 0, 0];
    $te = [$seed++, mt_rand(30, 120), 0, 0];
    $tm = [$seed++, mt_rand(30, 200), 0, 0];
    $a = gen(...$ta);
    $e = gen(...$te);
    $m = gen(...$tm);
    line('powmod', ['@' . implode(',', $ta), '@' . implode(',', $te), '@' . implode(',', $tm), 0], fn() => bcpowmod($a, $e, $m, 0));
}

