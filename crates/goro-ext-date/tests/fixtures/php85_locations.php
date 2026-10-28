<?php
date_default_timezone_set('UTC');
foreach (DateTimeZone::listIdentifiers() as $n) {
  $l = (new DateTimeZone($n))->getLocation();
  echo "L\t$n\t", $l['country_code'], "\t", var_export($l['latitude'], true), "\t", var_export($l['longitude'], true), "\t", $l['comments'], "\n";
}
foreach ([DateTimeZone::ALL] as $g) echo "ALL\t", implode(',', DateTimeZone::listIdentifiers($g)), "\n";
