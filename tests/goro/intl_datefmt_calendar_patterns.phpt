--TEST--
IntlDateFormatter: the date styles of non-Gregorian calendars come from CLDR's calendar data
--EXTENSIONS--
intl
--FILE--
<?php
// th_TH defaults to the Buddhist calendar, whose th patterns put the era in
// the full style only (and keep it with a Gregorian calendar set on the
// formatter); ja's Japanese calendar writes the era before the year; fa_IR
// uses fa's Persian patterns; calendars a locale has no data for go through
// root's aliases to the generic patterns (en islamic, de buddhist)
$ts = 1700000000;
foreach ([
    ["th_TH", IntlDateFormatter::TRADITIONAL, "Asia/Bangkok"],
    ["th_TH", IntlDateFormatter::GREGORIAN, "Asia/Bangkok"],
    ["ja_JP@calendar=japanese", IntlDateFormatter::TRADITIONAL, "Asia/Tokyo"],
    ["ja_JP@calendar=japanese", IntlDateFormatter::GREGORIAN, "Asia/Tokyo"],
    ["fa_IR", IntlDateFormatter::TRADITIONAL, "Asia/Tehran"],
    ["he_IL@calendar=hebrew", IntlDateFormatter::TRADITIONAL, "Asia/Jerusalem"],
    ["en_US@calendar=hebrew", IntlDateFormatter::TRADITIONAL, "UTC"],
    ["en_US@calendar=islamic-umalqura", IntlDateFormatter::TRADITIONAL, "UTC"],
    ["de_DE@calendar=buddhist", IntlDateFormatter::TRADITIONAL, "UTC"],
] as [$loc, $cal, $tz]) {
    echo $loc, " ", $cal, "\n";
    foreach ([IntlDateFormatter::FULL, IntlDateFormatter::LONG, IntlDateFormatter::MEDIUM, IntlDateFormatter::SHORT] as $d) {
        $f = new IntlDateFormatter($loc, $d, IntlDateFormatter::NONE, $tz, $cal);
        echo "  ", $f->getPattern(), " => ", $f->format($ts), "\n";
    }
}
?>
--EXPECT--
th_TH 0
  EEEEที่ d MMMM G y => วันพุธที่ 15 พฤศจิกายน พ.ศ. 2566
  d MMMM y => 15 พฤศจิกายน 2566
  d MMM y => 15 พ.ย. 2566
  d/M/yy => 15/11/66
th_TH 1
  EEEEที่ d MMMM G y => วันพุธที่ 15 พฤศจิกายน ค.ศ. 2023
  d MMMM y => 15 พฤศจิกายน 2023
  d MMM y => 15 พ.ย. 2023
  d/M/yy => 15/11/23
ja_JP@calendar=japanese 0
  Gy年M月d日EEEE => 令和5年11月15日水曜日
  Gy年M月d日 => 令和5年11月15日
  Gy年M月d日 => 令和5年11月15日
  GGGGGy/M/d => R5/11/15
ja_JP@calendar=japanese 1
  Gy年M月d日EEEE => 西暦2023年11月15日水曜日
  Gy年M月d日 => 西暦2023年11月15日
  Gy年M月d日 => 西暦2023年11月15日
  GGGGGy/M/d => AD2023/11/15
fa_IR 0
  y MMMM d, EEEE => 1402 آبان 24, چهارشنبه
  d MMMM y => 24 آبان 1402
  d MMM y => 24 آبان 1402
  y/M/d => 1402/8/24
he_IL@calendar=hebrew 0
  EEEE, d בMMMM y => יום רביעי, 2 בכסלו 5784
  d בMMMM y => 2 בכסלו 5784
  d בMMMM y => 2 בכסלו 5784
  d בMMMM y => 2 בכסלו 5784
en_US@calendar=hebrew 0
  EEEE, d MMMM y => Tuesday, 1 Kislev 5784
  d MMMM y => 1 Kislev 5784
  d MMM y => 1 Kislev 5784
  d MMM y => 1 Kislev 5784
en_US@calendar=islamic-umalqura 0
  EEEE, MMMM d, y G => Tuesday, Rabiʻ II 30, 1445 AH
  MMMM d, y G => Rabiʻ II 30, 1445 AH
  MMM d, y G => Rab. II 30, 1445 AH
  M/d/y GGGGG => 4/30/1445 AH
de_DE@calendar=buddhist 0
  EEEE, d. MMMM y G => Dienstag, 14. November 2566 BE
  d. MMMM y G => 14. November 2566 BE
  dd.MM.y G => 14.11.2566 BE
  dd.MM.yy GGGGG => 14.11.66 BE
