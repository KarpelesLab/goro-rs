<?php
// Largest N for which preg_match($p, str_repeat($unit, N)) does not fail (JIT on).
$cases = [['/(a)*/', 'a'], ['/(?:a)*$/', 'a'], ['/^(foo)+$/', 'foo'], ['/(.)*/s', 'a'], ['/((a))*/', 'a'], ['/(?:(a)|b)*/', 'a'],
  ['/(?:a|b)*?c/', 'a'], ['/(a|b)*c/', 'a'], ['/(?:ab)*$/', 'ab'], ['/(?:a(b))*$/', 'ab'], ['/(a)+$/', 'a'], ['/(?:a|b)*$/', 'a'], ['/(?>a)*$/', 'a'],
  ['/(a)*?$/', 'a'], ['/(?:a+)*$/', 'a'], ['/(?=a)*a*$/', 'a'], ['/(?:(?=a)a)*$/', 'a'], ['/((((a))))*$/', 'a'], ['/(a(?1)?)/', 'a'], ['/(?R)?a/', 'a']];
foreach ($cases as [$p, $u]) {
  $lo = 0; $hi = 4000000;
  if (@preg_match($p, str_repeat($u, $hi)) !== false) { echo "$p >$hi\n"; continue; }
  while ($lo < $hi) {
    $mid = intdiv($lo + $hi + 1, 2);
    if (@preg_match($p, str_repeat($u, $mid)) !== false) $lo = $mid; else $hi = $mid - 1;
  }
  @preg_match($p, str_repeat($u, $lo + 1));
  echo str_pad($p, 20), " $lo err=", preg_last_error(), "\n";
}
