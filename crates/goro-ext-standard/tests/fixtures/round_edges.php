<?php
// Driver for round_edges.txt: round() at extreme precisions, floats as big-endian bit patterns.
function h($v) { return bin2hex(pack('E', $v)); }
foreach ([-2.5, -1234.5678, -0.0001, -1e20, -7.0, -0.65, -1e-5, -123456789.0, -1e300] as $v) {
  $line = "neg " . h($v) . ":";
  for ($k = 1; $k <= 40; $k++) $line .= " " . h(round($v, -$k));
  foreach ([-100, -300, -307, -308, -309, -310, -320, -400, -1000] as $p) $line .= " " . h(round($v, $p));
  echo $line, "\n";
}
foreach ([5.045, 7.0, 0.61, 0.123456789, 1.5, 123.456, 9.5, 0.3, 1e-5, 4503599627370495.0, 2251799813685247.5, 1e-300, 5e-324, 2.2250738585072014e-308] as $v) {
  foreach ([1, 2, 3, 4] as $m) {
    $line = "pos " . h($v) . " $m:";
    for ($p = 10; $p <= 30; $p++) $line .= " " . h(round($v, $p, $m));
    foreach ([100, 290, 300, 308, 310, 320, 323, 324, 330, 400] as $p) $line .= " " . h(round($v, $p, $m));
    echo $line, "\n";
  }
}
$ps = array_merge(range(-20, -45), range(-290, -310));
echo "ps:", implode(' ', $ps), "\n";
foreach ([2e23, -2e23, 1e300, -1e300, 5e299, 1e25, -1e25, 3e40, -3e40, 4e40, 6e299, 1.5e300, 1e301, 1e305] as $v) {
  $line = "neg " . h($v) . ":";
  foreach ($ps as $p) $line .= " " . h(round($v, $p));
  echo $line, "\n";
}
