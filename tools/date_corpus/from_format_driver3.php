<?php
// createFromFormat / date_parse_from_format fixture driver (goro-ext-date
// tests/fixtures/from_format_php85.tsv). One line per case:
//   zone \t format \t input \t date_parse_from_format canon \t createFromFormat canon [\t now_s \t now_us0 \t now_us1]
// Strings are raw when printable ASCII (and not starting with '%'), else '%' + hex.
// Messages are '~N' (index into $MSG) when known.
$MSG = ['Trailing data', 'Not enough data available to satisfy format', 'Unexpected data found.',
  'The timezone could not be found in the database', 'The format separator does not match',
  'The parsed time was invalid', 'The parsed date was invalid', 'A two digit second could not be found',
  'Hour cannot be higher than 12', 'A textual day could not be found', 'A two digit hour could not be found',
  'A four digit year could not be found', 'A textual month could not be found', 'A meridian could not be found',
  'A two digit minute could not be found', 'The separation symbol could not be found',
  'A two digit day could not be found', 'Found unexpected data', 'Number out of range',
  'A two digit month could not be found', 'A six digit microsecond could not be found',
  'A three digit millisecond could not be found', 'A two digit year could not be found',
  'A three digit day-of-year could not be found', 'The separation symbol ([;:/.,-]) could not be found',
  "A 'day of year' can only come after a year has been found", 'The escaped character could not be found'];
$cases = [];
function add($f, $inputs, $zone = 'UTC') {
  global $cases;
  foreach ((array)$f as $ff) foreach ($inputs as $i) $cases[] = [$zone, $ff, $i];
}
add(["!D", "D"], ["day","days","week","weeks","fortnight","sec","msec","usec","year","month","hour","min","tues","mons","Mon;x","Mon'x","wed.","WEEKDAYS","Mon]","Mon[","Mon@","Mon*","Mon+","Mon\\","Mon|"]);
add(["!e", "!T"], ["Europe/Paris'", "EST'", "Europe/Paris;", "Europe/Paris]", "EST]", "Europe/Paris@", "EST;", "(EST", "((EST))", "(EST))", " ( EST ) ", "\t(\tEST"]);
add(["!e e", "!T T"], ["EDT Foo", "Europe/Paris Foo", "CEST Foo", "+01:00 Europe/Paris", "EST +01:00", "Europe/Paris +01:00", "UTC EST", "EST UTC", "Foo EST", "Foo Europe/Paris"]);
add(["!e e e"], ["EST +01:00 Europe/Paris", "EDT Foo Europe/Paris", "Europe/Paris EST Foo"]);
add(["D!", "D |", "D|", "!D !", "D Y!"], ["Mon", "Mon ", "Mon 2021"]);
add(["!g a", "!a"], ["1 a.x", "1 p.m", "1 a.m.", "1 a.M", "1 A.m.", "1 aM.", "1 pmpm", "a", "p", "am", ".m.", "1 a."]);
add(["U", "!U"], ["00000000000000000000001", "0000000000000000000000000000001", "+00000000000000000001"]);
add(["!S j", "!jS", "S"], ["ST1", "Th 1", "1sT", "1nD x", "1rd", "1Rd"]);
add(["!\\\xc3\xa9Y", "!Y\\\xc3"], ["\xc3\xa92021", "2021\xc3\xa9", "2021", "x2021"]);
add(["Y-m X", "!Y X", "Y-m-d x", "!Y-m U", "Y-m-d H:i:s U"], ["2021-06 x", "2021 -", "2021-06-15 +", "2021-06 99999999999999999999", "2021-06-15 10:20:30 x"]);
add(["D e", "S", "e", "D|", "+", "?", "*"], ["Mon Europe/Paris", "st", "Europe/Paris", "Mon", "x", "y", "z"], 'Pacific/Kiritimati');
add(["!Y-m-d H:i:s", "Y-m-d H:i:s", "!Y-m-d"], ["2021-03-28 02:30:00", "2021-10-31 02:30:00", "2021-10-31 01:30:00", "2021-10-31 03:30:00"], 'Europe/Berlin');
add(["!Y-m-d H:i:s", "!Y-m-d H:i:s e"], ["2021-10-31 02:30:00 Europe/Paris", "2021-11-07 01:30:00 America/New_York", "2021-10-31 01:30:00 Europe/London", "2021-04-04 02:30:00 Australia/Sydney"], 'UTC');
add(["!m", "!M", "!F"], ["Jan5", "Jan-", "Jan,", "5Jan", "\tJan", "Sept.", "sept", "i", "ii", "vi", "Oct", "xiii", "IIII", "vV"]);
add(["!d M Y H:i", "!M d, Y g:i A", "!l, F jS, Y", "!D, d M Y H:i:s O"], ["15 Jun 2021 10:30", "Jun 15, 2021 3:05 PM", "Tuesday, June 15th, 2021", "Tue, 15 Jun 2021 10:30:45 +0200", "Tue, 15 Jun 2021 10:30:45 -0000"]);
function enc($s) { return ($s !== '' && ($s[0] === '%' || preg_match('/[^\x20-\x7e]/', $s))) ? '%' . bin2hex($s) : $s; }
function msg($m) { global $MSG; $i = array_search($m, $MSG, true); return $i === false ? $m : "~$i"; }
function canon_list($a) { $o = []; foreach ($a as $k => $v) $o[] = "$k=" . msg($v); return implode(';', $o); }
function canon_we($w, $wc, $e, $ec) { return "$wc:" . canon_list($w) . "#$ec:" . canon_list($e); }
function canon_dp($p) {
  $o = fn($v) => $v === false ? 'F' : (string)$v;
  $f = [$o($p['year']), $o($p['month']), $o($p['day']), $o($p['hour']), $o($p['minute']), $o($p['second']),
        $p['fraction'] === false ? 'F' : (string)(int)round($p['fraction'] * 1e6),
        canon_we($p['warnings'], $p['warning_count'], $p['errors'], $p['error_count'])];
  $z = '';
  if ($p['is_localtime']) {
    $z = $p['zone_type'] . ',' . ($p['zone'] ?? '-') . ',' . (isset($p['is_dst']) ? (int)$p['is_dst'] : '-') . ',' . ($p['tz_abbr'] ?? '-') . ',' . ($p['tz_id'] ?? '-');
  }
  $f[] = $z;
  if (isset($p['relative'])) {
    $r = $p['relative']; $parts = [$r['year'], $r['month'], $r['day'], $r['hour'], $r['minute'], $r['second']];
    if (isset($r['weekday'])) $parts[] = 'wd=' . $r['weekday'];
    if (isset($r['weekdays'])) $parts[] = 'wds=' . $r['weekdays'];
    if (!empty($r['first_day_of_month'])) $parts[] = 'fdm';
    if (!empty($r['last_day_of_month'])) $parts[] = 'ldm';
    $f[] = implode(',', $parts);
  } else $f[] = '';
  $extra = array_diff(array_keys($p), ['year','month','day','hour','minute','second','fraction','warning_count','warnings','error_count','errors','is_localtime','zone_type','zone','is_dst','tz_abbr','tz_id','relative']);
  if ($extra) $f[] = 'EXTRA=' . implode(',', $extra);
  return implode('|', $f);
}
foreach ($cases as [$zone, $f, $in]) {
  if (strpos($in, "\0") !== false || strpos($f, "\0") !== false) continue;
  $p = date_parse_from_format($f, $in);
  $dp = canon_dp($p);
  $dpwe = canon_we($p['warnings'], $p['warning_count'], $p['errors'], $p['error_count']);
  $tz = new DateTimeZone($zone);
  for ($try = 0; $try < 5; $try++) {
    $t0 = microtime(false); $r = DateTime::createFromFormat($f, $in, $tz); $le = DateTime::getLastErrors(); $t1 = microtime(false);
    [$u0, $s0] = explode(' ', $t0); [$u1, $s1] = explode(' ', $t1);
    if ($s0 === $s1) break;
  }
  $les = $le === false ? 'false' : canon_we($le['warnings'], $le['warning_count'], $le['errors'], $le['error_count']);
  if ($les === $dpwe || ($les === 'false' && $dpwe === '0:#0:')) $les = '=';
  if ($r === false) $cr = 'F|' . $les;
  else {
    $tt = json_decode(json_encode(['t' => $r]), true)['t']['timezone_type'];
    $cr = 'O|' . $r->format('Y-m-d H:i:s.u') . '|' . $r->getTimezone()->getName() . '|' . $tt . '|' . $r->format('U') . '|' . $les;
  }
  $line = $zone . "\t" . enc($f) . "\t" . enc($in) . "\t$dp\t$cr";
  if (!($f !== '' && $f[0] === '!')) $line .= "\t$s0\t" . (int)round($u0 * 1e6) . "\t" . (int)round($u1 * 1e6);
  echo $line, "\n";
}
// ---- getLastErrors state probes (lines start with '#', not compared)
$show = function ($l) { echo "#\t", json_encode(DateTime::getLastErrors()), "\t$l\n"; };
$fail = function () { DateTime::createFromFormat('Y', 'x'); };
$fail(); $show('after failing cff');
$fail(); date_parse_from_format('Y', '2021'); $show('after ok dpff');
$fail(); date_parse_from_format('Y', 'x'); $show('after failing dpff');
$fail(); date_parse('xx'); $show('after date_parse');
$fail(); strtotime('xx'); $show('after strtotime');
$fail(); new DateTime('now'); $show('after new DateTime now');
$fail(); new DateTime(); $show('after new DateTime()');
$fail(); date_create('xx'); $show('after date_create fail');
$fail(); date_create('2021-02-30'); $show('after date_create 02-30');
$fail(); date_create_immutable('2021-02-30'); $show('after date_create_immutable 02-30');
$fail(); try { new DateTime('xx'); } catch (Throwable $e) { echo "#\t", get_class($e), ': ', $e->getMessage(), "\n"; } $show('after new DateTime throw');
$fail(); try { new DateTimeImmutable('xx'); } catch (Throwable $e) {} $show('after new DateTimeImmutable throw');
$d = new DateTime('2021-01-01');
$fail(); try { $d->modify('xx'); } catch (Throwable $e) { echo "#\t", get_class($e), ': ', $e->getMessage(), "\n"; } $show('after modify throw');
$fail(); $d->modify('+1 day'); $show('after modify ok');
$fail(); date_modify($d, '+1 day'); $show('after date_modify ok');
$fail(); try { $r = @date_modify($d, 'xx'); echo "#\tdate_modify returned ", json_encode($r), "\n"; } catch (Throwable $e) { echo "#\t", get_class($e), ': ', $e->getMessage(), "\n"; } $show('after date_modify fail');
$fail(); DateTime::createFromFormat('Y', '2021'); $show('after ok cff');
DateTime::createFromFormat('Y+', '2021x'); $show('after warn cff');
DateTime::createFromFormat('!Y-m-d', '2021-02-30'); $show('after cff invalid date (warning only)');
$fail(); DateTimeImmutable::createFromFormat('Y', '2021'); $show('after ok immutable cff');
$fail(); echo "#\t", json_encode(DateTimeImmutable::getLastErrors()), "\timmutable getLastErrors\n";
$fail(); echo "#\t", json_encode(date_get_last_errors()), "\tdate_get_last_errors\n";
$fail(); new DateTimeZone('UTC'); $show('after DateTimeZone');
$fail(); date_create_from_format('Y', '2021'); $show('after date_create_from_format ok');
$fail(); $i = new DateTimeImmutable('2021-01-01'); $show('after new DTI ok');
$fail(); $i->modify('+1 day'); $show('after immutable modify ok');
$fail(); $i->setDate(2021, 2, 30); $show('after setDate');
$fail(); $i->setTimezone(new DateTimeZone('UTC')); $show('after setTimezone');
$fail(); try { new DatePeriod('R2/2012-07-01T00:00:00Z/P7D'); } catch (Throwable $e) {} $show('after DatePeriod iso');
$fail(); DateTime::createFromInterface(new DateTimeImmutable('2021-01-01')); $show('after createFromInterface');
$fail(); $x = DateTime::__set_state(['date' => '2021-01-01 00:00:00.000000', 'timezone_type' => 3, 'timezone' => 'UTC']); $show('after __set_state');
$fail(); date_default_timezone_set('UTC'); $show('after date_default_timezone_set');
$fail(); $dd = new DateTime('@0'); $show('after new DateTime @0');
foreach ([['DateTime::createFromFormat', "Y", "20\0"], ['DateTime::createFromFormat', "Y\0", "20"], ['DateTimeImmutable::createFromFormat', "Y\0", "20"], ['date_create_from_format', "Y\0", "20"], ['date_create_immutable_from_format', "Y", "2\0"], ['date_parse_from_format', "Y\0", "20"], ['date_parse_from_format', "Y", "20\0"]] as [$fn, $f, $s]) {
  try { $r = $fn($f, $s); echo "#\t$fn returned ", json_encode($r), "\n"; } catch (Throwable $e) { echo "#\t", get_class($e), ': ', $e->getMessage(), "\n"; }
}
var_dump(DateTime::createFromFormat('Y', 'x'));
var_dump(date_create_from_format('Y', 'x'));
var_dump(DateTimeImmutable::createFromFormat('Y', 'x'));
print_r(date_parse_from_format('!Y-m-d e', '2021-06-15 Europe/Paris'));
print_r(date_parse_from_format('D', 'Mon'));
print_r(date_parse_from_format('U', '0'));
print_r(DateTime::getLastErrors());
DateTime::createFromFormat('\\', 'AAAABBBB'); print_r(DateTime::getLastErrors());
echo "#\t", timezone_version_get(), "\n";
