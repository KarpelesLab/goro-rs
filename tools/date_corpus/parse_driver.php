<?php
// One run over a corpus of strings: date_parse() output, strtotime()
// against fixed bases and modify() results.
date_default_timezone_set('UTC');
$in = json_decode(file_get_contents($argv[1]), true);
$out = fopen($argv[2], 'w');
$warn = null;
set_error_handler(function ($no, $str) use (&$warn) { $warn = $str; return true; });
$b1 = new DateTimeImmutable('2021-06-15 13:45:56.654321', new DateTimeZone('Europe/Paris'));
$b2 = new DateTimeImmutable('2020-02-29 23:30:15', new DateTimeZone('America/New_York'));
$b3 = new DateTimeImmutable('2021-01-31 00:00:00', new DateTimeZone('UTC'));
foreach ($in as $s) {
  $p = date_parse($s); foreach (['warnings','errors'] as $k) { $l = []; foreach ($p[$k] as $pos => $msg) $l[] = [$pos, $msg]; $p[$k] = $l; } $r = ['s' => $s, 'p' => $p];
  date_default_timezone_set('UTC');
  $t = strtotime($s, 1623764756);
  $r['t1'] = $t;
  date_default_timezone_set('America/New_York');
  $r['t2'] = strtotime($s, 1582950615);
  date_default_timezone_set('UTC');
  foreach (['m1' => $b1, 'm2' => $b2, 'm3' => $b3] as $k => $b) {
    $warn = null;
    try {
      $m = $b->modify($s);
      $r[$k] = $m === false ? ['false', $warn] : $m->format('Y-m-d H:i:s.u T e U');
    } catch (Throwable $e) {
      $r[$k] = [get_class($e), $e->getMessage(), $warn];
    }
  }
  try {
    $d = new DateTimeImmutable($s);
    $r['c'] = 'ok';
  } catch (Throwable $e) {
    $r['c'] = $e->getMessage();
  }
  fwrite($out, json_encode($r, JSON_INVALID_UTF8_SUBSTITUTE | JSON_PARTIAL_OUTPUT_ON_ERROR) . "\n");
}
