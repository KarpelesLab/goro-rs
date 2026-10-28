<?php
// Run once on PHP 8.5 to record compressed outputs:
//   php deflate_driver.php > deflate_php85.txt
// Each line is "case<TAB>output", output being hex for short results and
// "length:md5" otherwise (see out_repr()).
require __DIR__ . '/gen_data.php';

$inputs = inputs();
$levels = [-1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

// One-shot functions at every level.
foreach ($inputs as $name => $data) {
    foreach ($levels as $l) {
        echo "gzdeflate/$name/$l\t", out_repr(gzdeflate($data, $l)), "\n";
    }
    foreach ([0, 1, 6, 9] as $l) {
        echo "gzcompress/$name/$l\t", out_repr(gzcompress($data, $l)), "\n";
        echo "gzencode/$name/$l\t", out_repr(gzencode($data, $l)), "\n";
    }
}

// Strategies at a few levels, through deflate_init.
$strategies = [ZLIB_DEFAULT_STRATEGY, ZLIB_FILTERED, ZLIB_HUFFMAN_ONLY, ZLIB_RLE, ZLIB_FIXED];
foreach ($inputs as $name => $data) {
    foreach ($strategies as $s) {
        foreach ([0, 1, 4, 6, 9] as $l) {
            $c = deflate_init(ZLIB_ENCODING_RAW, ['level' => $l, 'strategy' => $s]);
            echo "strategy/$name/$s/$l\t", out_repr(deflate_add($c, $data, ZLIB_FINISH)), "\n";
        }
    }
}

// Memory levels and window sizes.
$some = ['hello', 'text5000', 'mixed400k', 'period_text', 'runs200k', 'zeros100k', 'rand70000', 'textend', 'x263', 'fib'];
foreach ($some as $name) {
    $data = $inputs[$name];
    foreach ([1, 2, 3, 4, 5, 6, 7, 8, 9] as $m) {
        foreach ([0, 1, 6, 9] as $l) {
            $c = deflate_init(ZLIB_ENCODING_RAW, ['level' => $l, 'memory' => $m]);
            echo "memory/$name/$m/$l\t", out_repr(deflate_add($c, $data, ZLIB_FINISH)), "\n";
        }
    }
    foreach ([8, 9, 10, 11, 12, 13, 14, 15] as $w) {
        foreach ([ZLIB_ENCODING_RAW, ZLIB_ENCODING_DEFLATE, ZLIB_ENCODING_GZIP] as $e) {
            foreach ([0, 1, 6, 9] as $l) {
                $c = @deflate_init($e, ['level' => $l, 'window' => $w]);
                if ($c === false) { echo "window/$name/$w/$e/$l\tfalse\n"; continue; }
                $r = @deflate_add($c, $data, ZLIB_FINISH);
                echo "window/$name/$w/$e/$l\t", $r === false ? 'false' : out_repr($r), "\n";
            }
        }
    }
}

// Incremental compression: chunk sizes x flush modes x levels. Records the
// concatenated output and the sequence of per-call output lengths.
$flushes = [ZLIB_NO_FLUSH, ZLIB_PARTIAL_FLUSH, ZLIB_SYNC_FLUSH, ZLIB_FULL_FLUSH, ZLIB_BLOCK];
foreach (['a15', 'text5000', 'mixed400k', 'rand70000', 'runs200k', 'text70k'] as $name) {
    $data = $inputs[$name];
    foreach ([1, 7, 100, 1000, 16384, 40000, 100000] as $chunk) {
        if ($chunk < 100 && strlen($data) > 6000) continue;
        foreach ($flushes as $f) {
            foreach ([0, 1, 6, 9] as $l) {
                foreach ([ZLIB_ENCODING_RAW, ZLIB_ENCODING_GZIP] as $e) {
                    if ($e == ZLIB_ENCODING_GZIP && $l != 6) continue;
                    $c = deflate_init($e, ['level' => $l]);
                    $out = '';
                    $lens = [];
                    for ($i = 0; $i < strlen($data); $i += $chunk) {
                        $o = deflate_add($c, substr($data, $i, $chunk), $f);
                        $out .= $o;
                        $lens[] = strlen($o);
                    }
                    $o = deflate_add($c, '', ZLIB_FINISH);
                    $out .= $o;
                    $lens[] = strlen($o);
                    echo "incr/$name/$chunk/$f/$l/$e\t", out_repr($out), " ", md5(implode(',', $lens)), "\n";
                }
            }
        }
    }
}

// Small hand-made call sequences, recorded call by call.
$seqs = [
    'empty-finish' => [['', ZLIB_FINISH]],
    'empty-noflush' => [['', ZLIB_NO_FLUSH], ['', ZLIB_NO_FLUSH], ['', ZLIB_FINISH]],
    'sync-twice' => [['abc', ZLIB_SYNC_FLUSH], ['', ZLIB_SYNC_FLUSH], ['', ZLIB_SYNC_FLUSH], ['def', ZLIB_FINISH]],
    'sync-full' => [['abc', ZLIB_SYNC_FLUSH], ['', ZLIB_FULL_FLUSH], ['', ZLIB_PARTIAL_FLUSH], ['', ZLIB_SYNC_FLUSH], ['abc', ZLIB_FINISH]],
    'partial-twice' => [['abcabc', ZLIB_PARTIAL_FLUSH], ['', ZLIB_PARTIAL_FLUSH], ['x', ZLIB_PARTIAL_FLUSH], ['', ZLIB_FINISH]],
    'block' => [['abcabcabc', ZLIB_BLOCK], ['', ZLIB_BLOCK], ['abc', ZLIB_BLOCK], ['', ZLIB_SYNC_FLUSH], ['abc', ZLIB_FINISH]],
    'block-full' => [['hello hello', ZLIB_BLOCK], ['hello', ZLIB_FULL_FLUSH], ['hello', ZLIB_FINISH]],
    'finish-twice' => [['abc', ZLIB_FINISH], ['', ZLIB_FINISH], ['def', ZLIB_FINISH]],
    'after-finish' => [['abc', ZLIB_FINISH], ['def', ZLIB_NO_FLUSH], ['', ZLIB_FINISH]],
    'noflush-finish' => [['hello', ZLIB_NO_FLUSH], [' world', ZLIB_NO_FLUSH], ['', ZLIB_FINISH]],
    'full-repeat' => [['aaaa', ZLIB_FULL_FLUSH], ['aaaa', ZLIB_FULL_FLUSH], ['aaaa', ZLIB_FINISH]],
];
foreach ($seqs as $sname => $seq) {
    foreach ([ZLIB_ENCODING_RAW, ZLIB_ENCODING_DEFLATE, ZLIB_ENCODING_GZIP] as $e) {
        foreach ([0, 1, 6] as $l) {
            $c = deflate_init($e, ['level' => $l]);
            $parts = [];
            foreach ($seq as [$d, $f]) {
                $r = deflate_add($c, $d, $f);
                $parts[] = $r === false ? 'false' : bin2hex($r);
            }
            echo "seq/$sname/$e/$l\t", implode('|', $parts), "\n";
        }
    }
}

// Dictionaries.
$dicts = [
    'az' => range('a', 'z'),
    'str' => 'hello world compression',
    'text' => text_data(3, 2000),
    'big' => text_data(17, 40000),
];
foreach ($dicts as $dname => $dict) {
    foreach (['hello', 'text5000', 'textend', 'a15'] as $name) {
        foreach ([ZLIB_ENCODING_RAW, ZLIB_ENCODING_DEFLATE, ZLIB_ENCODING_GZIP] as $e) {
            foreach ([0, 1, 6, 9] as $l) {
                try {
                    $c = @deflate_init($e, ['level' => $l, 'dictionary' => $dict]);
                    if ($c === false) { echo "dict/$dname/$name/$e/$l\tfalse\n"; continue; }
                    $r = @deflate_add($c, $inputs[$name], ZLIB_FINISH);
                    echo "dict/$dname/$name/$e/$l\t", $r === false ? 'false' : out_repr($r), "\n";
                } catch (Throwable $t) {
                    echo "dict/$dname/$name/$e/$l\t", get_class($t), ': ', $t->getMessage(), "\n";
                }
            }
        }
    }
}
