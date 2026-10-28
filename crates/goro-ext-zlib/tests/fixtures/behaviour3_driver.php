<?php
// Third batch of PHP 8.5 observations (run once):
//   php behaviour3_driver.php > behaviour3_php85.txt
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
$dir = sys_get_temp_dir() . '/goro-zlib3-' . getmypid();
@mkdir($dir);
$f = "$dir/t.gz";

// gzfile() on data without newlines and with binary bytes.
foreach (['zlib' => gzcompress('zlib data'), 'nul' => "a\0b\nc", 'cr' => "a\rb\r\nc", 'big' => gzencode($inputs['mixed400k'])] as $n => $d) {
    file_put_contents($f, $d);
    run("gzfile/$n", fn() => array_map(fn($l) => strlen($l) > 100 ? md5($l) . strlen($l) : bin2hex($l), array_slice(gzfile($f), 0, 20)));
    run("file/$n", fn() => array_map(fn($l) => strlen($l) > 100 ? md5($l) . strlen($l) : bin2hex($l), array_slice(file("compress.zlib://$f"), 0, 20)));
}
// Writing nothing.
foreach (['w', 'w0', 'wT'] as $m) {
    run("gzempty/$m", function () use ($f, $m) { $h = gzopen($f, $m); gzclose($h); return bin2hex(file_get_contents($f)); });
    run("gzempty-write0/$m", function () use ($f, $m) { $h = gzopen($f, $m); gzwrite($h, ''); gzclose($h); return bin2hex(file_get_contents($f)); });
    run("gzempty-flush/$m", function () use ($f, $m) { $h = gzopen($f, $m); fflush($h); gzclose($h); return bin2hex(file_get_contents($f)); });
}
// Level 0 through the deflate filter.
function filt_write(string $filter, $params, array $chunks, bool $flush = false) {
    $fp = fopen('php://memory', 'w+');
    $f = stream_filter_append($fp, $filter, STREAM_FILTER_WRITE, $params);
    $lens = [];
    $prev = 0;
    foreach ($chunks as $c) {
        fwrite($fp, $c);
        if ($flush) fflush($fp);
        $pos = ftell($fp); $lens[] = $pos - $prev; $prev = $pos;
    }
    stream_filter_remove($f);
    $lens[] = ftell($fp) - $prev;
    rewind($fp);
    return [stream_get_contents($fp), $lens];
}
foreach ([[1000, false], [1000, true], [40000, false], [40000, true], [100000, false], [70000, true]] as [$chunk, $fl]) {
    foreach ([0, 6] as $level) {
        run("filter0/$chunk/" . ($fl ? 'f' : 'n') . "/$level", function () use ($inputs, $chunk, $fl, $level) {
            [$o, $lens] = filt_write('zlib.deflate', ['level' => $level], str_split($inputs['mixed400k'], $chunk), $fl);
            return out_repr($o) . ' ' . md5(implode(',', $lens));
        });
    }
}
run('filter/close-with-data', function () use ($inputs) {
    $fp = fopen('php://memory', 'w+');
    stream_filter_append($fp, 'zlib.deflate', STREAM_FILTER_WRITE, ['level' => 0]);
    fwrite($fp, substr($inputs['mixed400k'], 0, 70000));
    $s = '';
    fclose($fp);
    return 'ok';
});
// Several invalid options at once.
run('filter/all-bad', function () { $fp = fopen('php://memory', 'w+'); return stream_filter_append($fp, 'zlib.deflate', STREAM_FILTER_WRITE, ['level' => 99, 'window' => 99, 'memory' => 99]) !== false; });
run('filter/inflate-bad', function () { $fp = fopen('php://memory', 'w+'); return stream_filter_append($fp, 'zlib.inflate', STREAM_FILTER_WRITE, ['window' => 99, 'level' => 99]) !== false; });
run('deflate_init/all-bad', fn() => deflate_init(ZLIB_ENCODING_RAW, ['level' => 99, 'memory' => 99, 'window' => 99, 'strategy' => 99]));
run('deflate_init/mem-window-bad', fn() => deflate_init(ZLIB_ENCODING_RAW, ['memory' => 99, 'window' => 99, 'strategy' => 99]));
run('deflate_init/window-strategy-bad', fn() => deflate_init(ZLIB_ENCODING_RAW, ['window' => 99, 'strategy' => 99]));
run('deflate_init/strategy-dict-bad', fn() => deflate_init(ZLIB_ENCODING_RAW, ['strategy' => 99, 'dictionary' => 5]));
run('deflate_init/enc-and-level-bad', fn() => deflate_init(99, ['level' => 99]));
run('inflate_init/window-dict-bad', fn() => inflate_init(ZLIB_ENCODING_RAW, ['window' => 99, 'dictionary' => 5]));
run('gzcompress/both-bad', fn() => gzcompress('x', 99, 99));
run('zlib_encode/both-bad', fn() => zlib_encode('x', 99, 99));
// Inflate filter reads in PHP's chunks.
run('filter/inflate/read-sizes', function () use ($inputs) {
    $fp = fopen('php://memory', 'w+');
    fwrite($fp, gzdeflate($inputs['text70k']));
    rewind($fp);
    stream_filter_append($fp, 'zlib.inflate', STREAM_FILTER_READ);
    $r = [];
    while (!feof($fp)) { $x = fread($fp, 30000); $r[] = strlen($x); }
    return implode(',', $r);
});
foreach (glob("$dir/*") as $x) unlink($x);
rmdir($dir);
