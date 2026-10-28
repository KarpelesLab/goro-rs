<?php
// Regex benchmark: php bench.php <corpus-out-dir>
// Writes the corpus (so the Rust side uses the same bytes) and prints
// "name<TAB>matches<TAB>ms" per pattern, timing preg_match_all() over the
// corpus (best of 5 runs) and a loop of short preg_match() calls.
$dir = $argv[1];
mt_srand(42);
$words = ['the', 'quick', 'brown', 'fox', 'jumps', 'over', 'lazy', 'dog', 'running', 'testing', 'apple', 'banana', 'cherry',
  'Lorem', 'ipsum', 'dolor', 'sit', 'amet', 'consectetur', 'adipiscing', 'elit', 'sed', 'do', 'eiusmod', 'tempor'];
$out = '';
while (strlen($out) < 1 << 20) {
    $r = mt_rand(0, 99);
    if ($r < 70) $out .= $words[mt_rand(0, count($words) - 1)];
    elseif ($r < 78) $out .= mt_rand(0, 99999);
    elseif ($r < 81) $out .= 'user' . mt_rand(1, 999) . '@example' . mt_rand(1, 9) . '.com';
    elseif ($r < 84) $out .= sprintf('%04d-%02d-%02d', mt_rand(1990, 2030), mt_rand(1, 12), mt_rand(1, 28));
    elseif ($r < 87) $out .= '<a href="http://example.com/' . mt_rand(1, 999) . '">link</a>';
    elseif ($r < 90) $out .= "\n# comment " . mt_rand(1, 99) . "\n";
    elseif ($r < 93) $out .= "café naïve résumé";
    else $out .= "\n";
    $out .= mt_rand(0, 9) == 0 ? ', ' : ' ';
}
file_put_contents("$dir/corpus.txt", $out);
$pats = [
    'literal' => '/fox/',
    'digits' => '/\d+/',
    'word_ing' => '/\b\w+ing\b/',
    'caseless' => '/LOREM|IPSUM/i',
    'email' => '/(\w+)@(\w+)\.com/',
    'date' => '/(\d{4})-(\d{2})-(\d{2})/',
    'href' => '/<a\s+href="([^"]*)"/i',
    'alternation' => '/\b(?:apple|banana|cherry)\b/',
    'comment_line' => '/^\s*#.*$/m',
    'spaces' => '/\s+/',
    'utf8_dot' => '/[^\x00-\x7f]+/u',
    'utf8_word' => '/\w+/u',
    'lazy_tag' => '/<a.*?>/',
    'not_found' => '/zzz\d+qqq/',
];
foreach ($pats as $name => $p) {
    $best = INF;
    $n = 0;
    for ($i = 0; $i < 5; $i++) {
        $t = hrtime(true);
        $n = preg_match_all($p, $out, $m);
        $best = min($best, (hrtime(true) - $t) / 1e6);
    }
    printf("%s\t%d\t%.3f\n", $name, $n, $best);
}
// Many short calls (per-call overhead).
$short = ['user42@example.com', 'not an email', 'a.b.c@d.org', 'x@y'];
$best = INF;
for ($i = 0; $i < 5; $i++) {
    $t = hrtime(true);
    $n = 0;
    for ($k = 0; $k < 100000; $k++) {
        $n += preg_match('/^[\w.+-]+@\w+\.[a-z]{2,}$/', $short[$k & 3]);
    }
    $best = min($best, (hrtime(true) - $t) / 1e6);
}
printf("short_calls\t%d\t%.3f\n", $n, $best);
