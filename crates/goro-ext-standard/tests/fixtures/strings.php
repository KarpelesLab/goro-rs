<?php
// Driver for strings.txt (one php85 run): name TAB args TAB result [TAB W:diag]*.
// Encoding: i:int, f:float bits (big-endian hex), s:hex string, b:bool, n:null,
// A{key=val;...} arrays (keys and values encoded the same way).
function enc($v) {
    if (is_int($v)) return "i:$v";
    if (is_float($v)) return "f:" . bin2hex(pack('E', $v));
    if (is_bool($v)) return "b:" . (int)$v;
    if (is_null($v)) return "n:";
    if (is_array($v)) { $o = "A{"; foreach ($v as $k => $x) $o .= enc($k) . "=" . enc($x) . ";"; return $o . "}"; }
    return "s:" . bin2hex($v);
}
function run($name, ...$args) {
    $ws = [];
    set_error_handler(function ($no, $str) use (&$ws) { $ws[] = $str; return true; });
    $byref = null;
    try {
        if ($name === 'str_replace' || $name === 'str_ireplace') { $r = $name($args[0], $args[1], $args[2], $cnt); $r = [$r, $cnt]; }
        elseif ($name === 'similar_text') { $r = similar_text($args[0], $args[1], $pct); $r = [$r, $pct]; }
        else $r = $name(...$args);
        $r = "R:" . enc($r);
    } catch (Throwable $t) { $r = "E:" . get_class($t) . ":" . $t->getMessage(); }
    restore_error_handler();
    foreach ($ws as $w) $r .= "\tW:$w";
    echo $name, "\t", implode(',', array_map('enc', $args)), "\t", $r, "\n";
}
$s = ['', 'a', 'abc', 'Hello World', "  \t\n padded \0\x0B ", 'The quick brown fox jumps over the lazy dog', "line1\nline2\r\nline3\rline4\n\r", "\xff\xfeé", 'aaa', 'xAx'];

foreach (['', 'abc', 'abcdef'] as $in) foreach ([-1, 0, 2, 3, 5, 8, 10] as $len) foreach ([' ', '*', 'xy', 'abc'] as $pad) foreach ([STR_PAD_RIGHT, STR_PAD_LEFT, STR_PAD_BOTH] as $t) run('str_pad', $in, $len, $pad, $t);
run('str_pad', 'abc', 10, '', STR_PAD_LEFT); run('str_pad', 'abc', 10, 'x', 3); run('str_pad', 'abc', 10, 'x', -1); run('str_pad', 'abc', 6);

$ww = ['The quick brown fox sat over the lazy dog', 'A very long woooooooooooord.', 'A very long woooooooooooooooooord. and something', '', 'short', "Line one\nLine two is long\n\nThree", 'abc  def   ghi', '  leading', 'trailing  ', 'x', "aaaa bbbb\ncccc"];
foreach ($ww as $in) foreach ([0, 1, 3, 5, 10, 15] as $w) foreach (["\n", '<br>', ''] as $brk) foreach ([false, true] as $cut) run('wordwrap', $in, $w, $brk, $cut);
run('wordwrap', 'abc def', 0, "\n", true); run('wordwrap', 'abc', -1, "\n", true); run('wordwrap', 'abc def', 3, '', true); run('wordwrap', 'abcdefghij', 3, "<br>\n", true); run('wordwrap', 'abc'); run('wordwrap', str_repeat('word ', 30));

foreach (['', 'abc', 'abcdefgh', "a\nb"] as $in) foreach ([1, 2, 3, 76] as $len) foreach (["\r\n", '|', '', 'ab'] as $e) run('chunk_split', $in, $len, $e);
run('chunk_split', 'abc', 0); run('chunk_split', 'abc', -1); run('chunk_split', 'abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghij');

foreach (["a\nb", "a\r\nb", "a\rb", "a\n\rb", "a\n\nb", "\r\r\n\n", "x", ''] as $in) { run('nl2br', $in); run('nl2br', $in, false); }
foreach ($s as $in) { run('strrev', $in); run('ucfirst', $in); run('lcfirst', $in); run('strtolower', $in); run('strtoupper', $in); run('ucwords', $in); }
foreach (['hello world-foo bar_baz', "hello\tworld\nfoo\rbar\fbaz\vqux", 'HELLO', 'élan vital', 'a-b-c', '-a-'] as $in) { run('ucwords', $in); run('ucwords', $in, '-'); run('ucwords', $in, ''); run('ucwords', $in, " -_"); run('lcfirst', $in); }

$wc = ["Hello fri3nd, you're looking good today!", "a-b c--d -e f- ' g' h'", "don't stop", '', "123 456", "héllo wörld", "a'b-c'd"];
foreach ($wc as $in) { foreach ([0, 1, 2] as $f) { run('str_word_count', $in, $f); run('str_word_count', $in, $f, '0..9'); run('str_word_count', $in, $f, "é3"); } }
run('str_word_count', 'abc', 3); run('str_word_count', 'abc', -1);

$tr = ["  abc  ", "\x00\t\n\x0B\r abc \x00\t\n\x0B\r", "xxabcxx", "abc", "", "aaa"];
foreach ($tr as $in) foreach ([null, 'x', 'a..c', 'a..', '..c', 'c..a', '..', 'a..b..c', "\x00..\x1f", 'a.c', 'x..x', '...', 'a...c', 'ab..', "z..a"] as $ch) {
  if ($ch === null) { run('trim', $in); run('ltrim', $in); run('rtrim', $in); }
  else { run('trim', $in, $ch); run('ltrim', $in, $ch); run('rtrim', $in, $ch); }
}

$sl = ["O'Rei\"lly\\", "a\0b", '', "\\\\'", "abc\\", "\\0\\x41\\n"];
foreach ($sl as $in) { run('addslashes', $in); run('stripslashes', $in); run('quotemeta', $in); run('stripcslashes', $in); }
foreach (["zoo['.']", "foo[bar]", "\x00\x1f\x7f\xff abc\n\t", "A..Z test", "abcdefghijklmnopqrstuvwxyz"] as $in) foreach (['A..Z', 'a..z', "\0..\37!@\177..\377", 'z..a', 'A..', '..Z', '.', 'a..b..d', "\n\t\r\v\f\a\b", ''] as $ch) run('addcslashes', $in, $ch);
foreach (['\\n\\t\\r\\v\\f\\a\\b\\\\', '\\x41\\x4\\x', '\\101\\0\\08\\1234', '\\q\\', 'abc\\', '\\e\\?', '\\x4G', '\\400'] as $in) run('stripcslashes', $in);
foreach (['1+1=2', '.\\+*?[^]$()', 'abc', ''] as $in) run('quotemeta', $in);

foreach (['Hi all, I said hello', 'abcabc', '', 'aaaa'] as $in) {
  run('strtr', $in, 'ai', 'eo'); run('strtr', $in, 'abc', 'x'); run('strtr', $in, 'a', 'xyz'); run('strtr', $in, '', 'x');
  run('strtr', $in, ['Hi' => 'Hello', 'hello' => 'hi', 'Hello' => 'X']); run('strtr', $in, ['a' => 'b', 'b' => 'a', 'ab' => 'X']); run('strtr', $in, ['' => 'x']); run('strtr', $in, []);
  run('strtr', $in, ['aa' => '1', 'a' => '2', 'aaa' => '3']); run('strtr', $in, [1 => 'one', 'c' => 'C']);
}

$sr = [['a', 'b', 'banana'], ['an', 'AN', 'banana'], ['A', 'x', 'bAnAna'], ['', 'x', 'abc'], [['a', 'b'], ['b', 'c'], 'ab'], [['a', 'b'], 'z', 'abc'], [['a', 'b', 'c'], ['1'], 'abcabc'], ['a', ['x'], 'abc'], ['l', 'L', ['hello', 'world', 5]], [['l', 'o'], ['L'], ['k' => 'hello', 3 => 'world']], ['aa', 'a', 'aaaaa'], ['ab', '', 'abab'], [[], 'x', 'abc'], [['', 'a'], 'x', 'aaa']];
foreach ($sr as $a) { run('str_replace', ...$a); run('str_ireplace', ...$a); }
run('str_ireplace', 'É', 'e', 'Éé'); run('str_ireplace', 'ab', 'x', 'AbaBAB');

foreach (['Hello', '', 'abcdef'] as $in) foreach ([0, 1, 3, -1, -3, 10, -10] as $st) foreach ([null, 0, 2, -1, -2, 10] as $ln) foreach (['XY', ''] as $rep) { if ($ln === null) run('substr_replace', $in, $rep, $st); else run('substr_replace', $in, $rep, $st, $ln); }
run('substr_replace', ['abc', 'defg'], 'X', 1, 1); run('substr_replace', ['abc', 'defg'], ['X', 'Y'], [0, 1], [1, 2]); run('substr_replace', ['abc', 'defg'], ['X'], [0, 1], [1]); run('substr_replace', 'abc', ['X'], 1); run('substr_replace', 'abc', 'X', [1]); run('substr_replace', ['a' => 'abc', 5 => 'xyz'], 'Q', 0, 0);

foreach (['hello hello hello', 'aaaa', 'abc', ''] as $h) foreach (['hello', 'aa', 'a', 'x'] as $n) foreach ([[], [1], [-5], [3, 5], [0, -2], [20], [-20], [2, 100], [0, 0]] as $extra) run('substr_count', $h, $n, ...$extra);
run('substr_count', 'abc', '');

$st = [['World', 'Word'], ['Hello', 'World'], ['', ''], ['abc', ''], ['PHP IS GREAT', 'WITH MYSQL'], ['aaaa', 'aa'], ['abcdef', 'fedcba'], ['bafoobar', 'barfoo'], ['abcxyzabc', 'xyzabcxyz'], ["\xff\x00", "\x00"]];
foreach ($st as [$a, $b]) { run('similar_text', $a, $b); run('similar_text', $b, $a); run('levenshtein', $a, $b); run('levenshtein', $a, $b, 2, 3, 4); run('levenshtein', $a, $b, 1, 0, 1); }
run('levenshtein', str_repeat('a', 300), str_repeat('b', 260));

$snd = ['Euler', 'Gauss', 'Hilbert', 'Knuth', 'Lloyd', 'Lukasiewicz', 'Ellery', 'Ghosh', 'Heilbronn', 'Kant', 'Ladd', 'Lissajous', 'Tymczak', 'Pfister', 'Ashcraft', 'Rubin', 'Rupert', '', '123', 'a', 'Wu', 'Honeyman', 'Burroughs', 'Burrows', 'Jackson', 'Schmidt', 'Shaw', 'O\'Hara', 'van Damme', "\xe9t\xe9", 'BbBb', 'Aeiou', 'HwH', 'Sgt Pepper'];
foreach ($snd as $in) { run('soundex', $in); run('metaphone', $in); }
$mp = ['Thompson', 'knight', 'gnome', 'pneumonia', 'wright', 'xavier', 'aesthetic', 'school', 'science', 'phone', 'ghost', 'laugh', 'dough', 'church', 'chemistry', 'ache', 'which', 'what', 'judge', 'edge', 'badge', 'nation', 'patio', 'shoe', 'sugar', 'tsar', 'thumb', 'lamb', 'MacBeth', 'yes', 'yay', 'hello world', 'Schwarzenegger', 'cia', 'ciao', 'scene', 'acid', 'black', 'quick', 'vixen', 'wax', 'zoo', 'Asia', 'vision', 'cough', 'Hugh', 'gh', 'agh', 'dumb', 'ckck', 'gin', 'gem', 'gym', 'giraffe', 'tough', 'bough', 'night', 'sight', 'ignite', 'signed', 'reign', 'dge', 'tch', 'witch', 'sch', 'schism', 'ch', 'xx', 'AE', 'WH', 'KN', 'GN', 'PN', 'WR', 'X', 'WHA', 'aaa', 'bbb', 'ccc', 'ss', 'tth', 'th', 'Ph', 'sci', 'sch', 'sio', 'sia', 'tia', 'tio', 'cy', 'ce', 'ck', 'ghi', 'gha', 'gn', 'gned', 'han', 'ahh', 'ehi', '1st', '-x-'];
foreach ($mp as $in) { run('metaphone', $in); }
foreach (['Thompson', 'knight', 'Schwarzenegger', 'abcdefghij'] as $in) foreach ([0, 1, 2, 3, 5] as $n) run('metaphone', $in, $n);
run('metaphone', 'abc', -1);

$nat = ['img12.png', 'img10.png', 'IMG2.png', 'img1.png', 'img2', 'img02', 'img002', 'a', 'A', '', '0', '00', '000', '1', '01', '-1', '1.5', '1.10', ' 1', '1 ', 'x 1', 'x  1', 'x2-g8', 'x2-y08', 'x2-y7', 'x8-y8', '0.1', '0.01', '0.10', 'a0b', 'a00b', 'abc', 'ABC', 'aBc', '1a', '1b', '10', '9', '0x1', 'Pic 0', 'pic 1', 'pic 01', 'pic02', 'pic2', 'pic 5 something', 'pic 5 ', '  leading', 'trail  '];
foreach ($nat as $a) foreach ($nat as $b) { run('strnatcmp', $a, $b); run('strnatcasecmp', $a, $b); }

foreach (['abc', '', 'Hello'] as $h) foreach (['', 'a', 'bc', 'abc', 'abcd', 'H', 'h', 'lo'] as $n) { run('str_contains', $h, $n); run('str_starts_with', $h, $n); run('str_ends_with', $h, $n); }
foreach (['This is a test text', 'abc', ''] as $h) foreach (['st', 'ex', 'z', 'cba', "\0"] as $c) run('strpbrk', $h, $c);
run('strpbrk', 'abc', '');
foreach (['42 is the answer', 'foo', '', 'aaabbb'] as $h) foreach (['1234567890', 'fo', 'a', ''] as $m) foreach ([[], [1], [-2], [1, 2], [0, -1], [10], [-10], [2, -10], [0, 0]] as $ex) { run('strspn', $h, $m, ...$ex); run('strcspn', $h, $m, ...$ex); }

foreach (['a,b,c', ',a,,b,', '', 'abc', 'a,b,c,d,e', ',,,'] as $in) foreach ([PHP_INT_MAX, 0, 1, 2, -1, -2, -10, 10] as $lim) foreach ([',', ',,', 'b'] as $sep) run('explode', $sep, $in, $lim);
run('explode', '', 'abc'); run('explode', ',', 'a,b', PHP_INT_MIN);
run('implode', ',', ['a', 'b', 'c']); run('implode', ',', []); run('implode', ['a', 'b']); run('implode', ', ', [1, 2.5, true, false, null, 'x', -0.0, 1e25, 0.1]);
foreach ($s as $in) { run('str_split', $in); run('str_split', $in, 3); run('str_word_count', $in); run('ucwords', $in); run('nl2br', $in); run('strrev', $in); run('str_rot13', $in); run('strlen', $in); }
run('str_split', 'abc', 0); run('str_repeat', 'ab', 3); run('str_repeat', 'ab', 0); run('str_repeat', 'ab', -1);
foreach (['abcdef', 'ab', ''] as $in) foreach ([0, 1, -1, -3, 3, 6, 7, -7] as $o) { run('substr', $in, $o); foreach ([0, 1, -1, 2, -10, 10] as $l) run('substr', $in, $o, $l); }
foreach (['hello world', 'abcabc', ''] as $h) foreach (['o', 'abc', 'z', 'l', ''] as $n) foreach ([0, 1, -1, -3, 5, 20, -20] as $o) { run('strpos', $h, $n, $o); run('strrpos', $h, $n, $o); run('stripos', $h, strtoupper($n), $o); run('strripos', $h, strtoupper($n), $o); }
foreach (['user@example.com', 'abc', ''] as $h) foreach (['@', 'e', 'z', ''] as $n) foreach ([false, true] as $b) { run('strstr', $h, $n, $b); run('stristr', $h, strtoupper($n), $b); run('strrchr', $h, $n, $b); }
foreach (['Hello', 'hello', 'Hellx', 'Hel', '', 'HELLO', 'a', 'b', "a\0", "a\0b"] as $a) foreach (['Hello', 'hello', 'Hel', '', 'a', "a\0"] as $b) { run('strcmp', $a, $b); run('strcasecmp', $a, $b); run('strncmp', $a, $b, 3); run('strncasecmp', $a, $b, 3); }
run('strncmp', 'a', 'b', -1); run('substr_compare', 'Hello', 'ello', 1); run('substr_compare', 'Hello', 'ELLO', 1, 4, true); run('substr_compare', 'Hello', 'x', 10); run('substr_compare', 'Hello', 'lo', -2); run('substr_compare', 'Hello', 'lo', -2, 0);
run('ucwords', 'hello_world-foo', '_-'); run('nl2br', "a\0\nb");
run('strrev', 'abc'); run('str_word_count', 'Hello World again', 1, ''); run('count_chars', 'abcab', 1); run('count_chars', 'abcab', 3); run('count_chars', 'abcab', 4); run('count_chars', 'ab', 5);
run('wordwrap', 'Mary had a little lamb', 4, "\n", false); run('wordwrap', 'Mary had a little lamb', 4, "\n", true);
run('similar_text', 'Hello', 'World'); run('number_format', 0.5);
run('strtr', 'abc', 'ab', ''); run('strtr', 'abc', ['a' => 1, 'b' => 2.5, 'c' => null]);
run('str_pad', 'x', 5, 'ab', STR_PAD_BOTH); run('str_pad', 'x', 6, 'ab', STR_PAD_BOTH);
run('nl2br', "\n\r\n\r");
