<?php
// Second batch of PHP 8.5 observations (run once):
//   php behaviour2_driver.php > behaviour2_php85.txt
// Same line format as behaviour_driver.php.
require __DIR__ . '/gen_data.php';

$diags = [];
set_error_handler(function ($no, $str) use (&$diags) {
    $k = match ($no) { E_WARNING => 'W', E_NOTICE => 'N', E_DEPRECATED => 'D', default => "E$no" };
    $diags[] = "$k:$str";
    return true;
});
function val($v): string {
    if (is_string($v)) return 's:' . out_repr($v);
    if (is_object($v)) return 'o:' . get_class($v);
    return var_export($v, true);
}
function run(string $name, callable $f): void {
    global $diags;
    $diags = [];
    try {
        $r = $f();
        $res = '=> ' . val($r);
    } catch (Throwable $t) {
        $res = 'E:' . get_class($t) . ':' . $t->getMessage();
    }
    echo $name, "\t", implode(' | ', array_merge($diags, [$res])), "\n";
}
$inputs = inputs();

// deflate_add's output buffer: a level 0 stored block shows how much output
// space each call has (memory 2 keeps the stored block threshold low).
foreach ([990, 1000, 1001, 1100, 1500, 2000, 3000, 5000, 9999, 10000, 20000, 31000] as $n) {
    foreach ([500, 30] as $pre) {
        run("guess/$n/$pre", function () use ($n, $pre, $inputs) {
            $c = deflate_init(ZLIB_ENCODING_RAW, ['level' => 0, 'memory' => 2]);
            $a = deflate_add($c, substr($inputs['text300k'], 0, $pre), ZLIB_NO_FLUSH);
            $b = deflate_add($c, substr($inputs['text300k'], 1000, $n), ZLIB_NO_FLUSH);
            $d = deflate_add($c, '', ZLIB_FINISH);
            return strlen($a) . ',' . strlen($b) . ',' . strlen($d) . ',' . md5($a . $b . $d);
        });
    }
}
// Sync flushes that exactly fill deflate_add's buffer.
foreach ([1, 2, 3, 10, 50, 64, 100, 200] as $n) {
    run("syncfill/$n", function () use ($n) {
        $c = deflate_init(ZLIB_ENCODING_RAW, ['level' => 0]);
        $o = [];
        for ($k = 1; $k <= 70; $k++) $o[] = bin2hex(deflate_add($c, str_repeat('x', $n + $k), ZLIB_SYNC_FLUSH));
        return md5(implode(',', $o)) . ' ' . implode(',', array_map('strlen', $o));
    });
    run("syncfill6/$n", function () use ($n, $inputs) {
        $c = deflate_init(ZLIB_ENCODING_RAW, ['level' => 6]);
        $o = [];
        for ($k = 1; $k <= 70; $k++) $o[] = bin2hex(deflate_add($c, substr($inputs['rand70000'], $k * 100, $n + $k), ZLIB_SYNC_FLUSH));
        return md5(implode(',', $o)) . ' ' . implode(',', array_map('strlen', $o));
    });
}
run('gzcompress/level0/sizes', function () use ($inputs) {
    $r = [];
    foreach ([0, 1, 100, 32767, 32768, 65535, 65536, 65540, 131070, 131071, 200000] as $n) $r[] = strlen(gzcompress(substr($inputs['mixed400k'], 0, $n), 0)) . '/' . md5(gzdeflate(substr($inputs['mixed400k'], 0, $n), 0));
    return implode(',', $r);
});

// inflate_add with ZLIB_BLOCK and other flush modes, call by call.
$streams = [
    'raw' => gzdeflate('hello world') . 'TRAILING',
    'zlib' => gzcompress('hello world') . 'TRAILING',
    'gzip' => gzencode('hello world') . 'TRAILING',
    'raw2' => (function () { $c = deflate_init(ZLIB_ENCODING_RAW); return deflate_add($c, 'first block ', ZLIB_FULL_FLUSH) . deflate_add($c, 'second', ZLIB_FINISH); })(),
    'zlib2' => (function () { $c = deflate_init(ZLIB_ENCODING_DEFLATE); return deflate_add($c, 'first block ', ZLIB_SYNC_FLUSH) . deflate_add($c, 'second', ZLIB_FINISH); })(),
];
foreach ($streams as $sname => $s) {
    foreach ([ZLIB_ENCODING_RAW => 'raw', ZLIB_ENCODING_DEFLATE => 'zlib', ZLIB_ENCODING_GZIP => 'gzip'] as $e => $en) {
        if (substr($sname, 0, strlen($en)) !== $en) continue;
        foreach ([1, 2, 3, 5, 7, 100] as $chunk) {
            foreach ([ZLIB_BLOCK, ZLIB_NO_FLUSH, ZLIB_FINISH, ZLIB_SYNC_FLUSH] as $f) {
                run("trace/$sname/$chunk/$f", function () use ($s, $e, $chunk, $f) {
                    $ctx = inflate_init($e);
                    $res = [];
                    for ($i = 0; $i < strlen($s); $i += $chunk) {
                        $r = inflate_add($ctx, substr($s, $i, $chunk), $f);
                        $res[] = ($r === false ? 'F' : bin2hex($r)) . '/' . inflate_get_status($ctx) . '/' . inflate_get_read_len($ctx);
                    }
                    foreach (['', '', 'x'] as $extra) {
                        $r = inflate_add($ctx, $extra, $f);
                        $res[] = ($r === false ? 'F' : bin2hex($r)) . '/' . inflate_get_status($ctx) . '/' . inflate_get_read_len($ctx);
                    }
                    return implode(',', $res);
                });
            }
        }
    }
}
// Mixed flush modes on one context.
run('trace/mixed-modes', function () {
    $s = gzcompress('hello world, hello world, hello world');
    $ctx = inflate_init(ZLIB_ENCODING_DEFLATE);
    $res = [];
    foreach ([[0, 3, ZLIB_BLOCK], [3, 5, ZLIB_NO_FLUSH], [8, 4, ZLIB_BLOCK], [12, 100, ZLIB_SYNC_FLUSH]] as [$o, $l, $f]) {
        $r = inflate_add($ctx, substr($s, $o, $l), $f);
        $res[] = ($r === false ? 'F' : bin2hex($r)) . '/' . inflate_get_status($ctx) . '/' . inflate_get_read_len($ctx);
    }
    return implode(',', $res);
});

// zlib_decode's format detection.
$cases = [
    'raw-looks-zlib' => "\x08\x1d\x00\xe2\xff" . str_repeat('A', 29) . "\x03\x00",
    'raw-78' => "\x78\x9c" . 'zz',
    'zlib-garbage' => "\x78\x9c\xff\xff",
    'gzip-garbage' => "\x1f\x8b\x08\x00garbage",
    'gzip-magic-only' => "\x1f\x8b",
    'one-1f' => "\x1f",
    'raw-1f8b-start' => "\x1f\x8b\x00\x00",
    'zlib-then-raw' => gzcompress('x') . 'junk',
];
foreach ($cases as $cname => $d) {
    foreach (['zlib_decode', 'gzinflate', 'gzuncompress', 'gzdecode'] as $fn) {
        run("detect/$cname/$fn", fn() => $fn($d));
    }
}
// max_length when the output ends exactly at the limit but the stream does not.
run('maxlen/exact-then-more', fn() => gzinflate(substr(gzdeflate(str_repeat('a', 100)), 0, -1), 100));
run('maxlen/data-error-after', fn() => gzinflate(gzdeflate('hello') . "\xff", 5));
run('maxlen/large', fn() => strlen(gzuncompress(gzcompress($GLOBALS['inputs']['mixed400k']), 400000)));
run('maxlen/large-1', fn() => gzuncompress(gzcompress($GLOBALS['inputs']['mixed400k']), 399999));
run('maxlen/zlib_decode', fn() => zlib_decode(gzencode('abcdef'), 3));

// gz file API.
$dir = sys_get_temp_dir() . '/goro-zlib-' . getmypid();
@mkdir($dir);
$f = "$dir/t.gz";
function gzw(string $f, string $mode, array $parts) {
    $h = gzopen($f, $mode);
    if ($h === false) return false;
    $r = [];
    foreach ($parts as $p) $r[] = is_array($p) ? (string)gzseek($h, $p[0], $p[1] ?? SEEK_SET) : gzwrite($h, $p);
    $r[] = gztell($h);
    gzclose($h);
    return implode(',', $r) . ' ' . bin2hex(file_get_contents($f));
}
foreach (['w', 'wb', 'w0', 'w1', 'w9', 'wf', 'wh', 'wR', 'wF', 'w9h', 'wT', 'a', 'ab', 'x', 'w+', 'r+', 'c', 'q', 'w5e', 'wb6f'] as $mode) {
    @unlink($f);
    run("gzopen/w/$mode", fn() => gzw($f, $mode, ['hello hello hello ', 'world']));
}
@unlink($f);
run('gzopen/append', function () use ($f) { gzw($f, 'w', ['first ']); return gzw($f, 'a', ['second']); });
run('gzopen/seek-write', fn() => gzw($f, 'w', ['abc', [10], 'def', [5], [20, SEEK_CUR], 'g', [0, SEEK_END]]));
run('gzopen/big-write', fn() => md5(gzw($f, 'w', str_split($GLOBALS['inputs']['mixed400k'], 3000))));
run('gzopen/big-write-w0', fn() => md5(gzw($f, 'w0', str_split($GLOBALS['inputs']['mixed400k'], 3000))));
run('gzopen/big-write-w1', fn() => md5(gzw($f, 'w1', str_split($GLOBALS['inputs']['text300k'], 70000))));
run('gzopen/flush', function () use ($f) {
    $h = gzopen($f, 'w');
    gzwrite($h, 'part1');
    fflush($h);
    gzwrite($h, 'part2');
    fflush($h);
    fflush($h);
    gzclose($h);
    return bin2hex(file_get_contents($f));
});
function gzr(string $f, string $mode, array $ops) {
    $h = gzopen($f, $mode);
    if ($h === false) return false;
    $r = [];
    foreach ($ops as $op) {
        switch ($op[0]) {
            case 'read': $x = gzread($h, $op[1]); $r[] = 'r:' . ($x === false ? 'F' : bin2hex($x)); break;
            case 'gets': $x = isset($op[1]) ? gzgets($h, $op[1]) : gzgets($h); $r[] = 'g:' . ($x === false ? 'F' : bin2hex($x)); break;
            case 'getc': $x = gzgetc($h); $r[] = 'c:' . ($x === false ? 'F' : bin2hex($x)); break;
            case 'seek': $r[] = 's:' . gzseek($h, $op[1], $op[2] ?? SEEK_SET); break;
            case 'tell': $r[] = 't:' . var_export(gztell($h), true); break;
            case 'eof': $r[] = 'e:' . var_export(gzeof($h), true); break;
            case 'rewind': $r[] = 'w:' . var_export(gzrewind($h), true); break;
            case 'pass': ob_start(); $n = gzpassthru($h); $r[] = 'p:' . $n . ':' . bin2hex(ob_get_clean()); break;
        }
    }
    gzclose($h);
    return implode(',', $r);
}
$files = [
    'gz' => gzencode("line one\nline two\nthird line here\n"),
    'plain' => "plain text\nnot compressed\n",
    'empty' => '',
    'two-members' => gzencode("first\n") . gzencode("second\n"),
    'member-garbage' => gzencode("first\n") . "garbage",
    'truncated' => substr(gzencode($inputs['text5000']), 0, 1000),
    'corrupt' => (function () { $s = gzencode('The quick brown fox jumps over the lazy dog'); $s[15] = 'X'; return $s; })(),
    'badcrc' => (function () { $s = gzencode('hello there'); $s[strlen($s) - 6] = 'X'; return $s; })(),
    'one-byte' => "\x1f",
    'magic-only' => "\x1f\x8b",
    'header-only' => substr(gzencode('abc'), 0, 10),
    'zlib' => gzcompress('zlib data'),
    'big' => gzencode($inputs['mixed400k']),
];
$ops = [
    'readall' => [['read', 100000], ['eof'], ['read', 10], ['eof'], ['tell']],
    'small' => [['read', 3], ['tell'], ['read', 5], ['tell'], ['eof']],
    'gets' => [['gets'], ['gets', 5], ['gets'], ['gets'], ['gets'], ['eof'], ['tell']],
    'getc' => [['getc'], ['getc'], ['tell'], ['seek', 0], ['getc']],
    'seek' => [['seek', 5], ['tell'], ['read', 4], ['seek', 2], ['read', 3], ['seek', 3, SEEK_CUR], ['read', 2], ['seek', -100, SEEK_CUR], ['tell'], ['seek', 1000], ['tell'], ['read', 5], ['eof'], ['seek', 0, SEEK_END], ['tell']],
    'rewind' => [['read', 6], ['rewind'], ['tell'], ['read', 6], ['pass']],
    'pass' => [['read', 2], ['pass'], ['eof'], ['pass']],
];
foreach ($files as $fname => $data) {
    file_put_contents($f, $data);
    foreach ($ops as $oname => $o) {
        if ($fname == 'big' && $oname != 'readall' && $oname != 'seek') continue;
        run("gzr/$fname/$oname", fn() => ($r = gzr($f, 'r', $o)) === false ? false : (strlen($r) > 2000 ? md5($r) : $r));
    }
    run("gzfile/$fname", fn() => ($r = gzfile($f)) === false ? false : md5(implode('|', $r)) . count($r));
    run("readgzfile/$fname", function () use ($f) { ob_start(); $n = readgzfile($f); $o = ob_get_clean(); return var_export($n, true) . ':' . md5($o); });
    run("compress.zlib/$fname", fn() => ($r = @file_get_contents("compress.zlib://$f")) === false ? false : md5($r) . strlen($r));
}
run('gzread/len0', function () use ($f) { file_put_contents($f, gzencode('abc')); $h = gzopen($f, 'r'); try { return gzread($h, 0); } finally { gzclose($h); } });
run('gzopen/missing', fn() => gzopen("$dir/missing.gz", 'r'));
run('gzopen/modes-r', function () use ($f) {
    file_put_contents($f, gzencode('abc'));
    $r = [];
    foreach (['r', 'rb', 'rt', 'r9', 'rf', 'rh', 're', 'rx', 'rT'] as $m) { $h = @gzopen($f, $m); $r[] = $m . '=' . ($h === false ? 'F' : gzread($h, 10)); if ($h) gzclose($h); }
    return implode(',', $r);
});
run('gzwrite/len', fn() => gzw($f, 'w', ['hello']) . '|' . (function () use ($f) { $h = gzopen($f, 'w'); $a = gzwrite($h, 'hello world', 5); $b = gzwrite($h, 'xyz', 0); $c = gzwrite($h, 'xyz', 100); $d = gzwrite($h, ''); gzclose($h); return "$a,$b,$c,$d," . bin2hex(file_get_contents($f)); })());
run('gzread/on-write', function () use ($f) { $h = gzopen($f, 'w'); $r = var_export(gzread($h, 10), true) . var_export(gzeof($h), true) . var_export(gzgets($h), true); gzclose($h); return $r; });
run('gzwrite/on-read', function () use ($f) { file_put_contents($f, gzencode('abc')); $h = gzopen($f, 'r'); $r = var_export(gzwrite($h, 'x'), true); gzclose($h); return $r; });
foreach (glob("$dir/*") as $x) unlink($x);
rmdir($dir);
