<?php
date_default_timezone_set('UTC');
$chars = 'dDjlNSwzWFmMntLoXxYyaABgGhHisuveIOPpTZcrU';
$dates = [[2021,7,4,13,5,9,123456],[1,1,1,0,0,0,0],[0,12,31,23,59,59,0],[-1,6,15,8,0,0,0],[-55,3,1,1,2,3,0],[787,1,1,0,0,0,0],[9999,12,31,23,59,59,0],[10000,1,1,0,0,0,0],[10191,5,5,12,0,0,0],[-10191,5,5,12,0,0,0],[2008,12,29,0,0,0,0],[2010,1,3,0,0,0,0],[2021,1,1,0,0,0,0],[2021,1,1,12,0,0,0],[2021,1,1,11,59,59,999999],[1970,1,1,0,0,0,0],[1969,12,31,23,59,59,500000],[2000,2,29,0,0,0,0],[1900,2,28,0,0,0,0],[2024,12,31,0,0,0,0],[2021,11,1,0,0,0,0],[2021,11,2,0,0,0,0],[2021,11,3,0,0,0,0],[2021,11,11,0,0,0,0],[2021,11,12,0,0,0,0],[2021,11,13,0,0,0,0],[2021,11,21,0,0,0,0],[2021,11,22,0,0,0,0],[2021,11,23,0,0,0,0],[2021,10,31,0,0,0,0],[-99,1,1,0,0,0,0],[-1000,1,1,0,0,0,0],[-9999,1,1,0,0,0,0],[-10000,1,1,0,0,0,0],[99,1,1,0,0,0,0],[-5,1,1,0,0,0,0],[5,1,1,0,0,0,0]];
foreach ($dates as $a) {
  $d = new DateTime('2000-01-01'); $d->setDate($a[0],$a[1],$a[2]); $d->setTime($a[3],$a[4],$a[5],$a[6]); $ds = implode(',', $a);
  echo $ds, "\n";
  foreach (str_split($chars) as $c) echo "  $c=", $d->format($c), "\n";
}
foreach (['Europe/Paris', 'America/New_York', 'Asia/Kolkata', 'Australia/Lord_Howe', 'Asia/Kathmandu', 'Africa/Monrovia', 'EST', 'EDT', 'CEST', '+05:30', '-03:00', '+05:30:15', '+00:00', 'Z', 'UTC', 'Pacific/Chatham', 'America/St_Johns'] as $z) {
  foreach (['1900-01-01 12:00', '1972-01-01 12:00', '2021-07-04 13:05:09', '2021-01-04 13:05:09'] as $ds) {
    $d = new DateTime($ds, new DateTimeZone($z));
    echo "$z $ds: ", $d->format('e|T|O|P|p|Z|I|c|r|U|B'), "\n";
  }
}
$d = new DateTime('2021-07-04 13:05:09.123456');
foreach (['\\Y\\m\\d', 'Y\\\\m', '\\', 'Y\\', '[Y]', 'Y-m-d H:i:s.u v', 'l jS \\of F Y h:i:s A', '%Y', 'D, d M Y', 'éY', "Y\0m", 'Ynd', 'q', 'k', 'J', 'b', 'C', 'E', 'f', 'K', 'Q', 'R', 'V'] as $f) echo json_encode($f), " => ", json_encode($d->format($f)), "\n";
foreach ([0, 1, -1, 86399, 86400, 3600, 1000000000, -1000000000, PHP_INT_MAX, PHP_INT_MIN, 253402300800, -62135596800, -62135596801, -62167219200, -62167219201] as $ts) echo $ts, " => ", date('c|r|D|N|z|W|o|Y|y|X|x|B', $ts), "\n";
echo DATE_ATOM, "\n", DATE_COOKIE, "\n", DATE_ISO8601, "\n", DATE_ISO8601_EXPANDED, "\n", DATE_RFC822, "\n", DATE_RFC850, "\n", DATE_RFC1036, "\n", DATE_RFC1123, "\n", DATE_RFC7231, "\n", DATE_RFC2822, "\n", DATE_RFC3339, "\n", DATE_RFC3339_EXTENDED, "\n", DATE_RSS, "\n", DATE_W3C, "\n";
foreach ([DATE_ATOM, DATE_COOKIE, DATE_ISO8601, DATE_ISO8601_EXPANDED, DATE_RFC822, DATE_RFC850, DATE_RFC1036, DATE_RFC1123, DATE_RFC7231, DATE_RFC2822, DATE_RFC3339, DATE_RFC3339_EXTENDED, DATE_RSS, DATE_W3C] as $f) echo $d->format($f), "\n";
$d = new DateTime('2021-07-04 13:05:09.123456', new DateTimeZone('America/New_York'));
echo $d->format(DATE_RFC7231), " ", $d->format(DATE_COOKIE), "\n";
