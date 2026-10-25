<?php
// usage: php dump_sbcs.php names.txt out.json
// For each glibc charset name, decides if it is a stateless single-byte
// charset and dumps its decode table and one-way encode mappings.
$names = file($argv[1], FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES);
$out = [];
foreach ($names as $cs) {
  $dec = [];
  $ok = true;
  $all = "";
  $expect = "";
  for ($b = 0; $b < 256; $b++) {
    error_clear_last();
    $r = @iconv($cs, "UTF-32BE", chr($b));
    if ($r === false) {
      $e = error_get_last();
      if ($e && str_contains($e['message'], 'incomplete')) { $ok = false; break; }
      if ($e && str_contains($e['message'], 'Wrong encoding')) { $ok = false; break; }
      $dec[] = -1;
      continue;
    }
    if (strlen($r) == 0) { $ok = false; break; } // shift byte
    if (strlen($r) != 4) { $ok = false; break; }
    $cp = unpack('N', $r)[1];
    if ($cp > 0xFFFE) { $ok = false; break; }
    $dec[] = $cp;
    $all .= chr($b);
    $expect .= $r;
  }
  if (!$ok) { fwrite(STDERR, "skip $cs\n"); continue; }
  // stateless: the whole string decodes as the concatenation, also reversed
  if (@iconv($cs, "UTF-32BE", $all) !== $expect) { fwrite(STDERR, "skip(state) $cs\n"); continue; }
  if (@iconv($cs, "UTF-32BE", strrev($all)) !== implode('', array_reverse(str_split($expect, 4)))) { fwrite(STDERR, "skip(state2) $cs\n"); continue; }
  $rev = [];
  foreach ($dec as $b => $cp) if ($cp >= 0 && !isset($rev[$cp])) $rev[$cp] = $b;
  $extra = [];
  $bad = false;
  for ($cp = 0; $cp < 0x10000; $cp++) {
    if ($cp >= 0xD800 && $cp < 0xE000) continue;
    $r = @iconv("UTF-32BE", $cs, pack('N', $cp));
    if ($r === false || $r === '') { if (isset($rev[$cp])) { $bad = true; } continue; }
    if (strlen($r) != 1) { $bad = true; break; }
    $b = ord($r);
    if (isset($rev[$cp]) && $rev[$cp] == $b) continue;
    $extra[] = [$cp, $b];
  }
  if ($bad) { fwrite(STDERR, "skip(enc) $cs\n"); continue; }
  $out[$cs] = ['dec' => $dec, 'extra' => $extra];
  fwrite(STDERR, "ok $cs extra=" . count($extra) . "\n");
}
file_put_contents($argv[2], json_encode($out));
