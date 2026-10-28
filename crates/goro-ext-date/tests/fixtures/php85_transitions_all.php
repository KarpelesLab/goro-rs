<?php
date_default_timezone_set('UTC');
$names = DateTimeZone::listIdentifiers(DateTimeZone::ALL_WITH_BC);
foreach ($names as $n) {
  try { $z = new DateTimeZone($n); } catch (Throwable $e) { continue; }
  $t = $z->getTransitions();
  if (!$t) continue;
  echo "Z $n\n";
  foreach ($t as $x) echo $x['ts'], ' ', $x['offset'], ' ', (int)$x['isdst'], ' ', $x['abbr'], "\n";
}
