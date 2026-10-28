<?php
// Measures mb_detect_encoding()'s per-code-point demerits for the BMP.
//
// Usage: php mb_detect_weights.php FROM TO > part.txt   (2-byte units FROM..TO-1)
//
// For each 2-byte unit T, the string is 200 copies of T followed by k
// copies of a padding unit. As UTF-16BE, T reads as the code point T; as
// UTF-16LE as T with its bytes swapped. The padding "\x00\x41" reads as 'A'
// in UTF-16BE and U+4100 in UTF-16LE; "\x41\x00" the other way round. Each
// line gives, for both paddings, the smallest k at which the result stops
// favouring one side and the smallest k at which it favours the other.
// tools/charset/detect_weights.py turns these thresholds into weights.
function cmp($s) {
  $a = mb_detect_encoding($s, ['UTF-16BE', 'UTF-16LE']);
  $c = mb_detect_encoding($s, ['UTF-16LE', 'UTF-16BE']);
  return ($a === $c) ? ($a === 'UTF-16BE' ? '<' : '>') : '=';
}
function first($Tm, $P, $pred) {
  if ($pred(cmp($Tm))) return 0;
  $lo = 0; $hi = 1;
  while (!$pred(cmp($Tm . str_repeat($P, $hi)))) { $lo = $hi; $hi *= 2; if ($hi > (1 << 20)) return -1; }
  while ($hi - $lo > 1) { $mid = ($lo + $hi) >> 1; if ($pred(cmp($Tm . str_repeat($P, $mid)))) $hi = $mid; else $lo = $mid; }
  return $hi;
}
$from = (int)$argv[1]; $to = (int)$argv[2];
for ($u = $from; $u < $to; $u++) {
  $Tm = str_repeat(pack('n', $u), 200);
  $k1 = first($Tm, "\x00\x41", fn($r) => $r !== '>');
  $k2 = first($Tm, "\x00\x41", fn($r) => $r === '<');
  $k3 = first($Tm, "\x41\x00", fn($r) => $r !== '<');
  $k4 = first($Tm, "\x41\x00", fn($r) => $r === '>');
  printf("%04X %d %d %d %d\n", $u, $k1, $k2, $k3, $k4);
}
