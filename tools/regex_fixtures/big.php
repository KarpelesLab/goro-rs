<?php
// Largest N that compiles for each template (N substituted for %d).
foreach (['/(a){1,%d}/', '/(?:a){1,%d}/', '/(?:ab|cd){%d}/', '/(a|b|c){1,%d}/', '/([a-z]){%d}/', '/(?:\d+\.){%d}/', '/(?:abcdefgh){%d}/', '/%s/', '/(?:[a-z]|é){%d}/u', '/(?:\p{L}){%d}/u'] as $t) {
  $lo = 1; $hi = 70000;
  $mk = fn($n) => $t === '/%s/' ? '/' . str_repeat('a', $n) . '/' : sprintf($t, $n);
  while ($lo < $hi) {
    $mid = intdiv($lo + $hi + 1, 2);
    if (@preg_match($mk($mid), '') !== false) $lo = $mid; else $hi = $mid - 1;
  }
  @preg_match($mk($lo + 1), '');
  echo str_pad($t, 24), " $lo ", error_get_last()['message'] ?? '', "\n";
}
