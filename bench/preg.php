<?php
// Regular expressions: match, match_all, replace, split, callbacks.
$text = str_repeat("The quick brown fox jumps over the lazy dog 12345 times, email me at fox@example.com or call 555-1234.\n", 200);
$t = hrtime(true);
$check = 0;
for ($round = 0; $round < 300; $round++) {
    $check += preg_match_all('/\b\w+\b/', $text, $m);
    $check += preg_match_all('/[a-z0-9._]+@[a-z0-9.]+\.[a-z]{2,}/i', $text, $m);
    $check += strlen(preg_replace('/\d+/', '#', $text));
    $check += count(preg_split('/[\s,.]+/', $text, -1, PREG_SPLIT_NO_EMPTY));
    $check += strlen(preg_replace_callback('/\b(\w)(\w*)\b/', fn($m) => strtoupper($m[1]) . $m[2], $text));
    for ($i = 0; $i < 2000; $i++) { $check += preg_match('/^(\d{3})-(\d{4})$/', "555-" . (1000 + $i), $mm); }
}
$ms = (hrtime(true) - $t) / 1e6;
printf("time_ms=%.3f check=%s\n", $ms, $check);
