<?php
// Run once on PHP 8.5 to record decoding results, warnings and exceptions:
//   php behaviour_driver.php > behaviour_php85.txt
// Each line is "case<TAB>result", where result lists the diagnostics raised
// (W: warning, N: notice, D: deprecation, E: exception) then "=> value".
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
$hello = 'hello world';

// ---- Argument validation.
foreach (['gzcompress', 'gzdeflate', 'gzencode'] as $fn) {
    foreach ([-2, -1, 10, 99] as $l) run("$fn/level/$l", fn() => $fn('abc', $l));
    foreach ([-15, 15, 31, 0, 1, 99, -1, 16] as $e) run("$fn/enc/$e", fn() => $fn('abc', 6, $e));
}
foreach ([-15, 15, 31, 0, 99] as $e) run("zlib_encode/enc/$e", fn() => zlib_encode('abc', $e));
foreach ([-2, 10] as $l) run("zlib_encode/level/$l", fn() => zlib_encode('abc', 15, $l));
foreach (['gzuncompress', 'gzinflate', 'gzdecode', 'zlib_decode'] as $fn) {
    run("$fn/maxlen/-1", fn() => $fn('abc', -1));
    run("$fn/empty", fn() => $fn(''));
    run("$fn/garbage", fn() => $fn('this is not compressed'));
    run("$fn/one", fn() => $fn("\x00"));
}

// ---- Decoding valid data with every function, and max_length.
$enc = [
    'raw' => fn($d) => gzdeflate($d),
    'zlib' => fn($d) => gzcompress($d),
    'gzip' => fn($d) => gzencode($d),
];
foreach (['empty', 'hello', 'text5000', 'mixed400k', 'zeros100k', 'a15'] as $name) {
    $d = $inputs[$name];
    foreach ($enc as $ename => $ef) {
        $c = $ef($d);
        foreach (['gzuncompress', 'gzinflate', 'gzdecode', 'zlib_decode'] as $fn) {
            run("decode/$name/$ename/$fn", fn() => $fn($c));
            $n = strlen($d);
            foreach ([1, $n - 1, $n, $n + 1] as $m) {
                if ($m < 1) continue;
                run("decode/$name/$ename/$fn/max$m", fn() => $fn($c, $m));
            }
        }
        // Truncations and trailing data.
        foreach ([1, 2, 5, 10, intdiv(strlen($c), 2), strlen($c) - 4, strlen($c) - 1] as $cut) {
            if ($cut <= 0 || $cut >= strlen($c)) continue;
            $t = substr($c, 0, $cut);
            foreach (['gzuncompress', 'gzinflate', 'gzdecode', 'zlib_decode'] as $fn) {
                run("trunc/$name/$ename/$cut/$fn", fn() => $fn($t));
            }
        }
        foreach (["\x00", "junk", $c] as $i => $tail) {
            foreach (['gzuncompress', 'gzinflate', 'gzdecode', 'zlib_decode'] as $fn) {
                run("tail/$name/$ename/$i/$fn", fn() => $fn($c . $tail));
            }
        }
    }
}

// ---- Corruptions: flip each byte of short streams.
foreach ($enc as $ename => $ef) {
    $c = $ef('The quick brown fox jumps over the lazy dog. The quick brown fox.');
    for ($i = 0; $i < strlen($c); $i++) {
        foreach ([0x01, 0x80, 0xff] as $x) {
            $b = $c;
            $b[$i] = chr(ord($b[$i]) ^ $x);
            foreach (['gzuncompress', 'gzinflate', 'gzdecode', 'zlib_decode'] as $fn) {
                if (($ename == 'raw') != ($fn == 'gzinflate') && $fn != 'zlib_decode') {
                    if (!($ename == 'gzip' && $fn == 'gzdecode') && !($ename == 'zlib' && $fn == 'gzuncompress')) continue;
                }
                run("flip/$ename/$i/$x/$fn", fn() => $fn($b));
            }
        }
    }
}

// ---- Hand-made streams.
$streams = [
    'stored-empty-final' => "\x01\x00\x00\xff\xff",
    'stored-badlen' => "\x01\x05\x00\xfa\xfe" . 'hello',
    'stored-short' => "\x01\x05\x00\xfa\xff" . 'hel',
    'fixed-empty' => "\x03\x00",
    'type3' => "\x07\x00",
    'nonfinal-only' => "\x02\x00",
    'dist-too-far' => hex2bin('4b0402'),   // literal 'a' + match dist 2?
    'gz-fname' => hex2bin('1f8b0808000000000003') . "name\0" . gzdeflate('hi') . pack('V', crc32('hi')) . pack('V', 2),
    'gz-fcomment' => hex2bin('1f8b0810000000000003') . "comment\0" . gzdeflate('hi') . pack('V', crc32('hi')) . pack('V', 2),
    'gz-fextra' => hex2bin('1f8b0804000000000003') . "\x03\x00abc" . gzdeflate('hi') . pack('V', crc32('hi')) . pack('V', 2),
    'gz-fhcrc-ok' => (function () { $h = hex2bin('1f8b0802000000000003'); return $h . pack('v', crc32($h) & 0xffff) . gzdeflate('hi') . pack('V', crc32('hi')) . pack('V', 2); })(),
    'gz-fhcrc-bad' => hex2bin('1f8b08020000000000030000') . gzdeflate('hi') . pack('V', crc32('hi')) . pack('V', 2),
    'gz-reserved-flag' => hex2bin('1f8b0820000000000003') . gzdeflate('hi') . pack('V', crc32('hi')) . pack('V', 2),
    'gz-badmethod' => hex2bin('1f8b0700000000000003') . gzdeflate('hi') . pack('V', crc32('hi')) . pack('V', 2),
    'gz-badcrc' => hex2bin('1f8b0800000000000003') . gzdeflate('hi') . pack('V', 0) . pack('V', 2),
    'gz-badlen' => hex2bin('1f8b0800000000000003') . gzdeflate('hi') . pack('V', crc32('hi')) . pack('V', 3),
    'gz-two-members' => gzencode('hello ') . gzencode('world'),
    'zlib-badcheck' => "\x78\x9d" . gzdeflate('hi') . pack('N', 0),
    'zlib-badmethod' => "\x79\x9c" . gzdeflate('hi'),
    'zlib-window16' => "\x88\x98" . gzdeflate('hi') . pack('N', adler32('hi')),
    'zlib-badadler' => "\x78\x9c" . gzdeflate('hi') . pack('N', 1),
    'zlib-fdict' => "\x78\xbb\x00\x00\x00\x01" . gzdeflate('hi') . pack('N', 1),
    'zlib-window9' => "\x18\x95" . gzdeflate('hi') . pack('N', adler32('hi')),
];
foreach ($streams as $sname => $s) {
    foreach (['gzuncompress', 'gzinflate', 'gzdecode', 'zlib_decode'] as $fn) {
        run("stream/$sname/$fn", fn() => $fn($s));
    }
}

function adler32(string $s): int { return hexdec(hash('adler32', $s)); }

// ---- zlib_decode on raw data and on gzip/zlib-looking prefixes.
foreach (['hello', 'text5000', 'a', 'allbytes'] as $name) {
    run("zlib_decode/raw/$name", fn() => zlib_decode(gzdeflate($inputs[$name])));
    foreach ([0, 1, 9] as $l) run("zlib_decode/raw/$name/$l", fn() => zlib_decode(gzdeflate($inputs[$name], $l)));
}

// ---- deflate_init / inflate_init options.
$opts = [
    'none' => [],
    'level-1' => ['level' => -1], 'level9' => ['level' => 9], 'level10' => ['level' => 10],
    'level-str' => ['level' => '5'], 'level-float' => ['level' => 5.5], 'level-null' => ['level' => null],
    'level-arr' => ['level' => []], 'level-bool' => ['level' => true], 'level-strbad' => ['level' => 'abc'],
    'memory0' => ['memory' => 0], 'memory1' => ['memory' => 1], 'memory9' => ['memory' => 9], 'memory10' => ['memory' => 10],
    'window7' => ['window' => 7], 'window8' => ['window' => 8], 'window9' => ['window' => 9], 'window15' => ['window' => 15], 'window16' => ['window' => 16],
    'window-15' => ['window' => -15], 'window31' => ['window' => 31],
    'strategy-1' => ['strategy' => -1], 'strategy0' => ['strategy' => 0], 'strategy4' => ['strategy' => 4], 'strategy5' => ['strategy' => 5],
    'dict-empty' => ['dictionary' => ''], 'dict-str' => ['dictionary' => 'abc'], 'dict-arr' => ['dictionary' => ['a', 'b']],
    'dict-arr-empty' => ['dictionary' => []], 'dict-arr-emptystr' => ['dictionary' => ['a', '']],
    'dict-arr-nul' => ['dictionary' => ["a\0b"]], 'dict-int' => ['dictionary' => 5], 'dict-null' => ['dictionary' => null],
    'dict-str-nul' => ['dictionary' => "a\0b"], 'dict-arr-int' => ['dictionary' => [1, 2]],
    'unknown' => ['foo' => 1], 'numeric-key' => [0 => 5],
];
foreach ($opts as $oname => $o) {
    foreach ([ZLIB_ENCODING_RAW, ZLIB_ENCODING_DEFLATE, ZLIB_ENCODING_GZIP] as $e) {
        run("deflate_init/$oname/$e", function () use ($e, $o) {
            $c = deflate_init($e, $o);
            if ($c === false) return false;
            return bin2hex(deflate_add($c, 'hello hello', ZLIB_FINISH));
        });
        run("inflate_init/$oname/$e", function () use ($e, $o) {
            $c = inflate_init($e, $o);
            if ($c === false) return false;
            $d = gzcompress('hello');
            return inflate_add($c, $e == ZLIB_ENCODING_RAW ? gzdeflate('hello') : ($e == ZLIB_ENCODING_GZIP ? gzencode('hello') : $d), ZLIB_FINISH);
        });
    }
}
foreach ([0, 1, -1, 16, 47, -16, 999] as $e) {
    run("deflate_init/enc/$e", fn() => deflate_init($e));
    run("inflate_init/enc/$e", fn() => inflate_init($e));
}

// ---- inflate_add incremental behaviour and statuses.
foreach (['hello', 'text5000', 'mixed400k'] as $name) {
    $d = $inputs[$name];
    foreach ([ZLIB_ENCODING_RAW => 'gzdeflate', ZLIB_ENCODING_DEFLATE => 'gzcompress', ZLIB_ENCODING_GZIP => 'gzencode'] as $e => $fn) {
        $c = $fn($d) . 'TRAILING';
        foreach ([1, 3, 64, 1000, 100000] as $chunk) {
            if ($chunk < 64 && strlen($c) > 20000) continue;
            foreach ([ZLIB_NO_FLUSH, ZLIB_SYNC_FLUSH, ZLIB_FINISH, ZLIB_PARTIAL_FLUSH, ZLIB_FULL_FLUSH, ZLIB_BLOCK] as $f) {
                run("inflate_add/$name/$e/$chunk/$f", function () use ($e, $c, $chunk, $f) {
                    $ctx = inflate_init($e);
                    $out = '';
                    $st = [];
                    $lens = [];
                    for ($i = 0; $i < strlen($c); $i += $chunk) {
                        $r = inflate_add($ctx, substr($c, $i, $chunk), $f);
                        if ($r === false) { $st[] = 'F'; break; }
                        $out .= $r;
                        $lens[] = strlen($r);
                        $st[] = inflate_get_status($ctx) . '/' . inflate_get_read_len($ctx);
                    }
                    return out_repr($out) . ' ' . md5(implode(',', $st)) . ' ' . md5(implode(',', $lens)) . ' ' . end($st);
                });
            }
        }
    }
}

// Status sequences on a tiny stream, call by call.
$seqs = [
    'bytewise' => [str_split(gzcompress('Hello world.') . 'junk'), ZLIB_NO_FLUSH],
    'bytewise-sync' => [str_split(gzcompress('Hello world.')), ZLIB_SYNC_FLUSH],
    'bytewise-finish' => [str_split(gzcompress('Hello world.')), ZLIB_FINISH],
    'half-finish' => [[substr(gzcompress('Hello world.'), 0, 8), substr(gzcompress('Hello world.'), 8)], ZLIB_FINISH],
    'empty-first' => [['', gzcompress('Hello world.'), ''], ZLIB_NO_FLUSH],
    'empty-finish' => [['', '', ''], ZLIB_FINISH],
    'garbage' => [['garbage!', 'more'], ZLIB_NO_FLUSH],
    'garbage-finish' => [['garbage!'], ZLIB_FINISH],
    'restart' => [[gzcompress('abc'), gzcompress('def'), gzcompress('ghi') . gzcompress('jkl')], ZLIB_NO_FLUSH],
    'restart-finish' => [[gzcompress('abc'), gzcompress('def')], ZLIB_FINISH],
    'after-error' => [['garbage!', gzcompress('abc')], ZLIB_NO_FLUSH],
    'sync-stream' => [[(function () { $c = deflate_init(ZLIB_ENCODING_DEFLATE); return deflate_add($c, 'part one ', ZLIB_SYNC_FLUSH); })(), (function () { $c = deflate_init(ZLIB_ENCODING_DEFLATE); deflate_add($c, 'part one ', ZLIB_SYNC_FLUSH); return deflate_add($c, 'part two', ZLIB_FINISH); })()], ZLIB_SYNC_FLUSH],
];
foreach ($seqs as $sname => [$parts, $f]) {
    run("inflate_seq/$sname", function () use ($parts, $f) {
        $ctx = inflate_init(ZLIB_ENCODING_DEFLATE);
        $res = [];
        foreach ($parts as $p) {
            $r = inflate_add($ctx, $p, $f);
            $res[] = ($r === false ? 'false' : bin2hex($r)) . '/' . inflate_get_status($ctx) . '/' . inflate_get_read_len($ctx);
        }
        return implode(',', $res);
    });
}

// Dictionaries.
$dict = 'hello world dictionary';
$cd = deflate_init(ZLIB_ENCODING_DEFLATE, ['dictionary' => $dict]);
$withdict = deflate_add($cd, 'hello world', ZLIB_FINISH);
$cr = deflate_init(ZLIB_ENCODING_RAW, ['dictionary' => $dict]);
$rawdict = deflate_add($cr, 'hello world', ZLIB_FINISH);
run('dict/zlib/ok', fn() => inflate_add(inflate_init(ZLIB_ENCODING_DEFLATE, ['dictionary' => $dict]), $withdict, ZLIB_FINISH));
run('dict/zlib/none', fn() => inflate_add(inflate_init(ZLIB_ENCODING_DEFLATE), $withdict, ZLIB_FINISH));
run('dict/zlib/wrong', fn() => inflate_add(inflate_init(ZLIB_ENCODING_DEFLATE, ['dictionary' => 'nope']), $withdict, ZLIB_FINISH));
run('dict/zlib/unneeded', fn() => inflate_add(inflate_init(ZLIB_ENCODING_DEFLATE, ['dictionary' => 'nope']), gzcompress('hello'), ZLIB_FINISH));
run('dict/raw/ok', fn() => inflate_add(inflate_init(ZLIB_ENCODING_RAW, ['dictionary' => $dict]), $rawdict, ZLIB_FINISH));
run('dict/raw/none', fn() => inflate_add(inflate_init(ZLIB_ENCODING_RAW), $rawdict, ZLIB_FINISH));
run('dict/gzip/ok', fn() => inflate_add(inflate_init(ZLIB_ENCODING_GZIP, ['dictionary' => $dict]), gzencode('hello'), ZLIB_FINISH));
run('dict/gzuncompress', fn() => gzuncompress($withdict));
run('dict/zlib_decode', fn() => zlib_decode($withdict));
run('dict/zlib/bytewise', function () use ($withdict, $dict) {
    $ctx = inflate_init(ZLIB_ENCODING_DEFLATE, ['dictionary' => $dict]);
    $o = '';
    foreach (str_split($withdict) as $ch) { $r = inflate_add($ctx, $ch); if ($r === false) return 'false'; $o .= $r; }
    return $o . '/' . inflate_get_status($ctx);
});
run('dict/zlib/none/bytewise', function () use ($withdict) {
    $ctx = inflate_init(ZLIB_ENCODING_DEFLATE);
    $o = [];
    foreach (str_split($withdict) as $ch) { $r = inflate_add($ctx, $ch); $o[] = var_export($r, true) . inflate_get_status($ctx); }
    return implode(',', $o);
});

// ---- deflate_add / inflate_add misc.
run('deflate_add/badflush', fn() => deflate_add(deflate_init(ZLIB_ENCODING_RAW), 'x', 99));
run('deflate_add/badflush-6', fn() => deflate_add(deflate_init(ZLIB_ENCODING_RAW), 'x', 6));
run('inflate_add/badflush', fn() => inflate_add(inflate_init(ZLIB_ENCODING_RAW), 'x', 99));
run('deflate_add/sync-noflush-sync', function () {
    $c = deflate_init(ZLIB_ENCODING_RAW);
    return bin2hex(deflate_add($c, 'abc', ZLIB_SYNC_FLUSH)) . ',' . bin2hex(deflate_add($c, '', ZLIB_NO_FLUSH)) . ',' . bin2hex(deflate_add($c, '', ZLIB_SYNC_FLUSH)) . ',' . bin2hex(deflate_add($c, 'x', ZLIB_SYNC_FLUSH)) . ',' . bin2hex(deflate_add($c, 'x', ZLIB_SYNC_FLUSH));
});
run('deflate_add/big-level0', function () use ($inputs) {
    $c = deflate_init(ZLIB_ENCODING_RAW, ['level' => 0]);
    $o = deflate_add($c, $inputs['mixed400k'], ZLIB_SYNC_FLUSH) . deflate_add($c, $inputs['rand70000'], ZLIB_NO_FLUSH) . deflate_add($c, '', ZLIB_FINISH);
    return out_repr($o);
});
run('gzcompress/level0/big', fn() => out_repr(gzcompress($inputs['mixed400k'], 0)));
foreach ([1, 2, 3, 4, 9] as $m) {
    run("deflate_add/level0/mem$m", function () use ($inputs, $m) {
        $c = deflate_init(ZLIB_ENCODING_RAW, ['level' => 0, 'memory' => $m]);
        $o = '';
        $lens = [];
        foreach (str_split($inputs['text70k'], 1000) as $p) { $r = deflate_add($c, $p, ZLIB_NO_FLUSH); $o .= $r; $lens[] = strlen($r); }
        $r = deflate_add($c, '', ZLIB_FINISH); $o .= $r; $lens[] = strlen($r);
        return out_repr($o) . ' ' . implode(',', $lens);
    });
}
run('inflate_get_status/fresh', fn() => inflate_get_status(inflate_init(ZLIB_ENCODING_RAW)));
run('inflate_get_read_len/fresh', fn() => inflate_get_read_len(inflate_init(ZLIB_ENCODING_RAW)));
run('zlib_get_coding_type', fn() => zlib_get_coding_type());

// ---- Stream filters (memory streams).
function filt_write(string $filter, $params, array $chunks, bool $flush = false) {
    $fp = fopen('php://memory', 'w+');
    $f = $params === null ? stream_filter_append($fp, $filter, STREAM_FILTER_WRITE) : stream_filter_append($fp, $filter, STREAM_FILTER_WRITE, $params);
    if ($f === false) return false;
    foreach ($chunks as $c) { fwrite($fp, $c); if ($flush) fflush($fp); }
    stream_filter_remove($f);
    rewind($fp);
    return stream_get_contents($fp);
}
function filt_read(string $filter, $params, string $data, int $n = 8192) {
    $fp = fopen('php://memory', 'w+');
    fwrite($fp, $data);
    rewind($fp);
    $f = $params === null ? stream_filter_append($fp, $filter, STREAM_FILTER_READ) : stream_filter_append($fp, $filter, STREAM_FILTER_READ, $params);
    if ($f === false) return false;
    $out = [];
    while (!feof($fp)) { $r = fread($fp, $n); $out[] = $r === false ? 'false' : out_repr($r); if (count($out) > 50) break; }
    return implode(',', $out);
}
$fparams = [
    'none' => null, 'level9' => ['level' => 9], 'level0' => ['level' => 0], 'level10' => ['level' => 10], 'level-2' => ['level' => -2],
    'window9' => ['window' => 9], 'window15' => ['window' => 15], 'window31' => ['window' => 31], 'window47' => ['window' => 47],
    'window-15' => ['window' => -15], 'window7' => ['window' => 7], 'window50' => ['window' => 50], 'memory1' => ['memory' => 1], 'memory10' => ['memory' => 10],
    'int' => 5, 'str' => 'abc', 'strategy' => ['strategy' => 2], 'obj' => (object)['level' => 1],
];
foreach ($fparams as $pname => $p) {
    run("filter/deflate/$pname", fn() => ($r = filt_write('zlib.deflate', $p, ['hello ', 'hello ', 'world'])) === false ? false : bin2hex($r));
    run("filter/deflate/$pname/flush", fn() => ($r = filt_write('zlib.deflate', $p, ['hello ', 'hello ', 'world'], true)) === false ? false : bin2hex($r));
}
run('filter/deflate/big', fn() => out_repr(filt_write('zlib.deflate', null, str_split($inputs['mixed400k'], 10000))));
run('filter/deflate/big/flush', fn() => out_repr(filt_write('zlib.deflate', ['level' => 6], str_split($inputs['text70k'], 7000), true)));
foreach (['none' => null, 'window15' => ['window' => 15], 'window31' => ['window' => 31], 'window47' => ['window' => 47], 'window-15' => ['window' => -15], 'window7' => ['window' => 7], 'window9' => ['window' => 9], 'window50' => ['window' => 50], 'int' => 5] as $pname => $p) {
    foreach (['raw' => gzdeflate('hello world'), 'zlib' => gzcompress('hello world'), 'gzip' => gzencode('hello world'), 'garbage' => 'garbage data', 'rawtail' => gzdeflate('hello world') . 'tail', 'trunc' => substr(gzdeflate($inputs['text5000']), 0, 500)] as $dname => $d) {
        run("filter/inflate/$pname/$dname/read", fn() => filt_read('zlib.inflate', $p, $d));
        run("filter/inflate/$pname/$dname/write", fn() => ($r = filt_write('zlib.inflate', $p, [$d])) === false ? false : out_repr($r));
    }
}
run('filter/inflate/big/read', fn() => filt_read('zlib.inflate', null, gzdeflate($inputs['mixed400k']), 100000));
run('filter/inflate/big/read-small', fn() => md5(filt_read('zlib.inflate', null, gzdeflate($inputs['text70k']), 1000)));
run('filter/inflate/two-streams', fn() => filt_read('zlib.inflate', null, gzdeflate('abc') . gzdeflate('def')));
