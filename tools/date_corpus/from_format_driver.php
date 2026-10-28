<?php
// createFromFormat / date_parse_from_format fixture driver (goro-ext-date
// tests/fixtures/from_format_php85.tsv). One line per case:
//   zone \t format \t input \t date_parse_from_format canon \t createFromFormat canon [\t now_s \t now_us0 \t now_us1]
// Strings are raw when printable ASCII (and not starting with '%'), else '%' + hex.
// Messages are '~N' (index into $MSG) when known. The create canon is
// 'F|' + getLastErrors() or 'O|date|zone name|zone type|U|' + getLastErrors(),
// '=' when getLastErrors() equals the date_parse_from_format() lists.
// Lines starting with '#' are getLastErrors() lifecycle probes.
//
// The fixture is this script's output plus from_format_driver3.php's (the
// follow-up questions), run once each on PHP 8.5 with
//   php -d error_reporting=-1 -d display_errors=1 -d log_errors=0
//       -d html_errors=0 -d date.timezone=UTC from_format_driver.php > run2
// and reduced below 400 KB by from_format_trim.py (fixture 2 run2 run3).
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
$num = ["0","1","01","001","12","13","23","24","25","29","30","31","32","59","60","61","99","100","366","999","2021","12345","-1","+1"," 1","1 ","a","","1a","1.5","06","9999999999","abc5","x5","--5"];
foreach (str_split("djmnYyGgHhisz") as $c) {
  add(["!$c"], $num);
  add(["!$c-"], ["1-", "12-", "123-", "1"]);
}
add(["!d|", "!H|"], ["1", "a", "", "1a"]);
add(["!Y z", "!y z", "!z Y", "!Y-z", "!Yz", "!yz", "!z", "!X z"], ["2021 0","2021 15","2021 364","2021 365","2021 366","2021 400","2021 1000","2020 59","2020 365","21 0","0 2021","15 2021","2021-15","202115","10153","15"]);
add(["!Y-m z", "!Y z m", "!m Y z", "!Y z d"], ["2021-06 10", "2021 10 06", "06 2021 10"]);
add(["z", "Y z", "z Y", "y z", "X z", "Y-m-d z", "d z", "Y!z", "Y|z", "U z"], ["100", "2021 100", "100 2021", "21 100", "x 15", "2021-06-15 10", "15 10", "2021 5", "0 5"]);
$txtd = ["Mon","mon","MON","Monday","monday","Mo","M","Tue","Tues","tuesday","Wed","Wednesday","Thu","Thur","Thursday","Fri","Friday","Sat","Saturday","Sun","Sunday","Mon.","Xyz","","1","Monx","Mondayx","Sundays","weekday","Tuesday 1",
  "Mon5","Mon_","Mon,","Mon:","Mon/","Mo-n","5Mon","Mondays","Tuesdays","weekdays","Sat.","Satu","Monday,","Mon-","Mon(","Mon)","Mon'","Mon\xc3\xa9"," Mon","\tMon","Mon Tue","last","next","first","third","12th"];
add(["!D", "!D|", "!D Y"], $txtd);
add(["!l"], ["Mon","Monday","Mo","weekdays","5Mon"]);
add(["D", "D H:i"], ["Mon","Sun","Wed","Mo","Tuesday 1","Mon."]);
add(["!Y-m-d D", "!D Y-m-d", "!Y-m-d l", "!Y-m-d H:i D", "!D Y-m-d H:i"], ["2021-06-14 Mon","2021-06-15 Mon","2021-06-13 Sun","2021-06-19 Sat","2021-06-20 Fri","Mon 2021-06-14","Tue 2021-06-14","Sat 2021-06-19","2021-06-14 10:00 Mon","Mon 2021-06-14 10:00","2021-06-15 10:00 Tue"]);
add(["!jS", "!j S", "!S", "!dS", "!jS M", "!S j"], ["1st","2nd","3rd","4th","1th","11st","1ST","1Nd","1xx","1","1s","21st Jan","1 st","st","22nd","23rd","31th","st 1","xx 1","1sT"]);
$mon = ["Jan","jan","JAN","January","january","Janu","Sept","Sep","September","Dec","XII","III","IV","XIII","Mar","May","Fo","Febr",""," Jan","Jan.","1","Janx","Januaryx","Juni","Jun","June","Jan5","Jan-","Jan,","5Jan","\tJan","Jan\xc3\xa9","Sept.","sept","i","ii","vi","Oct"];
add(["!M", "!F", "!M Y"], $mon);
add(["M"], ["Jan", "Febr", "Sept"]);
$mer = ["1:00 am","1:00 AM","1:00 pm","1:00 PM","12:00 am","12:00 pm","12:30 AM","12:30 PM","1:00 a.m.","1:00 p.m.","1:00 A.M.","1:00 P.M.","1:00 a.m","1:00 x","13:00 pm","13:00 am","0:00 am","0:00 pm","1:00 a","1:00 p","1:00","1:00 Am","1:00 aM","1:00 amx","11:59 pm","1:00  am","1:00am","1:00 am.","1:00 a.m.x","1:00 pm5","1:00 xam","1:00 a m","1:00 a.M.","1:00 p.m","1:00 x.m."];
add(["!g:i a", "!H:i A", "!g:i"], $mer);
add(["!a g", "!A h:i", "!a H", "!a G:i", "a g", "A H", "a", "g a", "a H:i"], ["am 1","pm 1","pm 1:00","am 12:30","pm 12:30","pm 13","am 13","pm 0","am 12","pm 12","pm 15:00","x 1","pm","1 pm","12 am"]);
add(["!g", "!H", "!g a", "!H a", "!a", "!A"], ["0","1","11","12","13","23","24","0 am","12 am","12 pm","13 pm","1 pm","am","pm","PM","a.m.","p.m.","x","0am","pmx"]);
$zones = ["UTC","utc","Europe/Paris","europe/paris","Europe/Paris ","EST","est","EDT","CEST","CET","Z","z","GMT","gmt","GMT+0200","GMT-05:00","GMT+5","+0200","+02:00","-0530","+5","+05","+0500","+05:00:30","+053030","+1:00","Foo/Bar","XYZ","America/New_York","America/Argentina/Buenos_Aires","(UTC)","(EST)","[Europe/Paris]","Etc/GMT+5","+02:00[Europe/Berlin]","","1","+","-","+0","-0000","+123","+12345","+1234567","A","J","PST","Pacific/Kiritimati","Europe/Paris1","EST5EDT","UTC+5","utc+05:00","Europe/Parisx y","Europe/Paris/x",
  "Europe/Paris)","(Europe/Paris)","(Europe/Paris"," Europe/Paris","  EST","\tEST","EST)","Europe/Paris,","Europe/Paris.","Europe-Paris","America/Port-au-Prince","America/Port_of_Spain","Etc/GMT-14","GMT+05:00:30","GMT+2","gmt+2","UTC-5","+02:00:30","+2:3","+:30","-5:","+0230 ","+05:00x","Z5","EST,","(","()","( UTC)","+02:","+0:00","+00:60","-12:45","+14","-14:00","Europe/Paris\xc3\xa9","Antarctica/Troll","America/Indiana/Knox","CHST","SAST","AEST","IST","WET","MSK","utc ","UT","Etc/UTC","Universal","Zulu","+1","-1","+12","+123456","+12:34:56","+1:2:3","GMT","GMT+","GMT-","GMT+x","GMTx","Europe/","/Paris","a/b"];
add(["!e"], $zones);
add(["!T", "!O", "!P", "!p"], ["UTC","EST","Europe/Paris","+02:00","+0200","Z","GMT+0200","Foo/Bar","(EST)","1","+12345"," EST"]);
add(["!e Y", "!O Y"], ["UTC 2021","EST 2021","+02:00 2021","Europe/Paris 2021","GMT+0200 2021","Z 2021","+0200 2021","Foo 2021","utc+05:00","Europe/Parisx y","[Europe/Paris]"]);
add(["!Y-m-d H:i:s T", "!Y-m-d H:i:s e"], ["2021-06-15 12:00:00 CEST","2021-06-15 12:00:00 CET","2021-01-15 12:00:00 CEST","2021-06-15 12:00:00 Europe/Paris","2021-06-15 12:00:00 +0200","2021-06-15 12:00:00 Z","2021-06-15 12:00:00 EDT","2021-06-15 12:00:00 America/New_York","2021-06-15 12:00:00 Foo"]);
add(["Y-m-d\\TH:i:sP[e]", "Y-m-d\\TH:i:sP e", "!P e", "!e P", "!e e", "!P P", "!T e", "!O T"], ["2022-02-18T00:00:00+01:00[Europe/Berlin]","2022-02-18T00:00:00+01:00 Europe/Berlin","+01:00 Foo","Foo +01:00","EST Europe/Paris","Europe/Paris EST","+01:00 +02:00","Foo Bar","EST Foo","+01:00 EST"]);
$uts = ["0","1","-1","1475500799","9999999999","1.5","+1","abc","","99999999999999999999","-86400","00012","1 "," 1","1e5","9223372036854775807","-9223372036854775808","9223372036854775808","-","+","--1","+-1"," -1","a-1","-a","- 1","1-"];
add(["U", "!U", "U|", "U.u"], $uts);
add(["U.u", "U.v", "U u"], ["1475500799.176312","1448889063.3531","1.8642","1.","1.1234567","1.123","1.1"]);
add(["U H", "H U", "!U Y", "Y U", "U e", "e U", "U T", "U Y-m-d", "Y-m-d U", "U !", "U|", "!U H:i", "U s", "U D", "U i", "U z", "U a"], ["3600 01","3600 05","01 3600","2021 3600","3600 2021","3600 Europe/Paris","Europe/Paris 3600","3600 +02:00","3600 EST","3600 2021-06-15","2021-06-15 3600","3600 ","3600 05:30","3600 Mon","3600 pm","3600 x"]);
add(["Y-m U", "!Y U", "Y U", "Y-m-d U", "Y-m-d H:i U", "Y-m-d H U", "Y-m-d\\TH:i:s U", "H:i:s U", "!Y-m-d U.u"], ["2021-06 x", "2021 -", "2021 99999999999999999999", "2021 +", "2021-06-15 x", "2021-06-15 10:30 x", "2021-06-15 10 x", "2021-06-15T10:30:00 x", "10:30:00 x", "10:30:00 99999999999999999999", "2021-06-15 abc.5"]);
add(["U e", "U P"], ["1623764756 Europe/Paris","1623764756 +02:00","1623764756 America/New_York"]);
$fr = ["1.","1.1","1.12","1.123","1.1234","1.12345","1.123456","1.1234567","1.12a","1.a","1.000000","1.001001","1.0010","1.999999","1.9999995","1. 5","1.-5","1.+5"];
add(["!s.u", "!s.v", "s.u"], $fr);
add(["!u", "!v", "u", "v", "!H u", "!uH", "!u.v", "!v.u"], ["0","1","12","123","1234","123456","1234567","000001","999","a","","1.2","12.34","x12"]);
$xs = ["2021","-2021","+2021","12345","+12345","-12345","0","999","+0","-0","21",""," 2021","+123456789","-0001","2021-06","+2021-06","10000","99999999999","9999999999999999999","99999999999999999999","-9223372036854775808","+-5","-+5","- 5","--5","x5","5x"];
add(["!X", "!x", "!X-m"], $xs);
add(["!y", "!y-m"], ["69","70","00","99","1","5","2021","123","05-06","5-06","1969","70-01","-1","+1"," 5"]);
$seps = [';', ':', '/', '.', ',', '-', '(', ')', ' ', '#', '?', '*', 'q', 'B', '@', '[', '+', '!', '|', "\t", "\n", "\x80"];
$sepin = [';', ':', '/', '.', ',', '-', '(', ')', ' ', 'a', '', '  ', "\t", '_', '[', '#', '..', '-/', 'q', 'B', "\n", "\x80", '9'];
foreach ($seps as $s) foreach ($sepin as $x) $cases[] = ['UTC', "!Y{$s}m", "2021{$x}06"];
add(["!Y m", "!Y  m", "!Y m ", "! Y m", "!Y m d", " !Y m", "!Y \t m"], ["2021 06","2021  06","2021\t06","202106","2021\n06","2021\r06","2021\v06","2021\f06"," 2021 06","2021 06 ","2021 06 07","2021 06  07","2021\x0006","2021 \t 06","\t2021 06"]);
add(["!Y*m", "!Y*-m", "!Y* m", "!*Y", "!Y-m*", "!Y*", "!*", "!Y**m", "!Y*?m"], ["2021abc06","2021abc-06","2021-06","2021 06","2021abc def06","2021abc,06","2021abc:06","2021ab.c06","abc 2021","2021-06abc","2021-06-abc","2021","2021abc","2021\xc3\xa906","2021 -06","2021abc(06","2021abc)06","2021abc\t06","2021ab#c06","2021abc;06","2021abc/06","2021abc_06","2021abc+06","2021abc\n06","2021abc'06","2021..06","2021--06"]);
add(["!Y?m", "!Y??m", "!?Y", "!Y?", "!Y?\\?m"], ["2021x06","2021-06","202106","2021\xc3\xa906","2021xx06","x2021","2021","20210","2021??06","2021"]);
add(["Y-m-d|", "Y-m-d!", "!Y", "|Y", "H|", "Y!m", "Y|m", "Y-m-d !H:i", "Y-m-d H:i!", "Y-m-d H:i|", "Y-m-d|H:i", "!Y-m-d|", "!", "|", "!!", "||", "", "!|", "Y-m-d+", "Y-m-d \\", "Y-m-d *", "Y-m-d ?", "Y-m-d #", "Y-m-d+ H", "Y-m-d H+", "Y-m-d |+", "Y-m-d!|", "Y-m-d e", "Y-m-d u", "Y-m-d ;"], ["2011-02-02","2011-02-02 10:30","2011","10","2011-06","2011-02-02 ","2011-02-0210:30","", "x"]);
add(["e !", "e|", "e!", "!e", "e !Y", "T!", "O|", "U!", "e U!", "!Y e !"], ["Europe/Paris ", "Europe/Paris", "EST", "+05:00", "3600", "Europe/Paris 3600", "Europe/Paris 2021"]);
add(["\\", "!\\", "!Y\\-m", "!\\Y Y", "!Y\\", "!Y\\\\m", "!\\YY", "!Y\\T", "!Y\\Tm", "!\\d d", "!Y\\ m", "!\\\\Y", "!\\\\", "!Y\\\\", "\\\\", "!\\\\\\Y Y", "!\\:Y", "!\\9Y", "!Y\\%"], ["2021-06","2021\\-06","Y 2021","2021\\06","Y2021","2021T","2021T06","d 05","2021 06","\\2021","AAAABBBB","2021","\\","",":2021","92021","2021%","2021x"]);
add(["Y-m-d", "Y-m-d+", "Y-m-d +", "+Y-m-d", "Y-m-d++", "Y+m", "!Y+", "!+", "+", "!m/d/y+", "!+m/d/y", "!m/d/y +", "!+Y-m-d H", "!Y-m-d+e"], ["2001-11-29 13:20:01","2001-11-29","2001-11-29 ","06/08/04 12:00","06/08/04","2001","200111","", "x", "2001-11-29 13 x", "2001-11-29 x"]);
add(["!Y-m-d H:i:s", "Y-m-d H:i:s"], ["2021-13-01 00:00:00","2021-02-30 00:00:00","2021-01-32 00:00:00","2021-00-00 00:00:00","2021-01-01 24:00:00","2021-01-01 25:61:61","2021-01-01 23:60:00","2021-01-01 23:59:60","2021-02-29 00:00:00","2020-02-29 00:00:00","2021-04-31 00:00:00","2021-06-15 99:99:99","0000-00-00 00:00:00","2021-12-31 23:59:59","2021-1-1 1:1:1","2021-06-15 12:00:00x","2021-06-15","2021-06-15 12","2021-06-15 12:","2021-06-15 12:00:","2021-06","2021-06-","2021","xx-06-15 12:00:00","2021-xx-15 12:00:00","2021/06/15 12:00:00","2021-06-15T12:00:00","2021-13-32 25:00:00","2021-02-29 24:00:00"]);
add(["m-d", "d", "m", "Y-m", "Y-d", "H", "i", "H:i", "s", "m-d H", "!m-d", "!d", "Y-m-d H:i"], ["02-29","02-30","06-31","13-01","32","13","2021-13","2021-32","25","61","23:60","00-00","0","02-29 25","2021-02-29 23:60"]);
add(["!d/m/Y", "!m/d/Y", "!j.n.Y", "!d-M-Y", "!m*d*y"], ["31/12/2021","12/31/2021","1.1.2021","01-Jan-2021","2021-06-15","06/08/04","xx/06/2021","06/xx/2021","8/8/2016","2009---01","2021-6-5","2021-006-05"]);
add(["!H:i", "!H:i:s", "!G:i", "!i", "!s"], ["25:00","24:00","23:60","12:30:61","99:99","-1:00","1:2","1:2:3","12:3","1:60:60"]);
add(["!Y-m-d H:i:s.u T", "m-d-Y H:i:s.u T"], ["03-15-2005 12:22:29.000000 PST","2005-03-15 12:22:29.001001 PST","03-15-2005 12:22:29.0010 PST","03-15-2005 12:22:29.001001 PST"]);
add(["D, d M Y H:i:s O", "l, d-M-Y H:i:s T", "Y-m-d\\TH:i:sP", "Y-m-d\\TH:i:s.vP", "D, d M y H:i:s O", "X-m-d\\TH:i:sP", "Ymd\\THis\\Z", "D M d H:i:s Y", "D., M# j, Y g:iA", "Y-m-d H:i:sO", "Y-m-d\\TH:i:sO", "x-m-d\\TH:i:sP", "l, d-M-y H:i:s T", "D, d M Y H:i:s T"], ["Tue, 08 Jul 2008 22:14:12 +0200","Tuesday, 08-Jul-2008 22:14:12 GMT+0200","2008-07-08T22:14:12+02:00","2008-07-08T22:14:12.000+02:00","Tue, 08 Jul 08 22:14:12 +0200","+2008-07-08T22:14:12+02:00","20170920T091600Z","Wed Aug  2 08:37:50 2023","Thu., Nov. 29, 2012 5:00PM","0021-08-21 00:00:00+00:49:56","Mon, 21-Jan-2041 15:24:52 GMT","Wed Aug 02 08:37:50 2023","Tuesday, 08-Jul-08 22:14:12 GMT+0200","2008-07-08T22:14:12+0200"]);
add(["aHa0", "s", "O", "!D d M Y", "!d M Y D", "D H i s", "z Y", "Y H:i:s z", "z H:i:s Y", "yz", "Y-m-d", "j-M-Y", "Y/m/d H:i:s", "!m/d/y", "!m*d*y", "m/d/y"], ["0=G9UCNnF","0","invalid","Fri 17 may 2011","17 may 2011 Fri","Tue 0 00 00","Tuesday 0 00 00","31 2020","60 2020","2016 12:00:00 15","16 12:00:00 2016","10153","2009---01","12-Sep-2013","1995/06/08 12:34:56","Fri 19 November 2011","Sat 19 November 2011","06/08/04","8/8/2016"]);
// now-dependent fills (UTC and a far zone)
foreach (['UTC', 'Pacific/Kiritimati', 'America/New_York'] as $z)
add(["Y", "m", "d", "H", "i", "s", "u", "v", "Y-m-d", "Y-m-d H", "H:i", "i:s", "Y-m-d H:i:s", "g a", "a", "A", "a g", "D", "l", "e", "T", "O", "y", "M", "z", "Y z", "jS", "S", "U", "", "X", "x", "H|", "Y-m-d H:i:s.u", "d H", "m s", "Y-m-d e", "H:i e", "e Y", "+", "*", "?", "#", "\\x", "D Y-m-d", "Y-m-d D", "U.u", "Y-m-d|", "H:i|", "Y|", "D|", "e|", "s.u", "u e"],
  ["2021","06","15","10","30","45","123456","123","2021-06-15","2021-06-15 10","10:30","30:45","2021-06-15 10:30:45","3 pm","pm","Mon","Europe/Paris","EST","+0200","21","Jun","100","2021 100","1st","3600","","st","2021-06-15 10:30:45.5","15 10","06 45","Pacific/Kiritimati","Etc/GMT+12","2021-06-15 Pacific/Kiritimati","10:30 Pacific/Kiritimati","Etc/GMT+12 2021","x","Mon 2021-06-15","2021-06-15 Mon","3600.5","45.5","5 Europe/Paris","pm 3"], $z);
// zones / DST
foreach (['Europe/Paris', 'America/New_York', 'Australia/Lord_Howe', 'EST', '+05:30', 'Pacific/Kiritimati', 'Europe/London', 'Australia/Sydney'] as $z) {
  add(["!Y-m-d H:i", "!Y-m-d H:i:s.u", "!Y-m-d H:i T", "!Y-m-d", "!Y-m-d H:i e", "!U", "Y-m-d H:i", "!D Y-m-d"],
    ["2021-03-28 02:30","2021-03-28 01:59","2021-03-28 03:00","2021-10-31 02:30","2021-10-31 01:59","2021-10-31 03:00","2021-10-31 02:00","2021-10-31 01:30","2021-03-14 02:30","2021-11-07 01:30","2021-11-07 00:59","2021-11-07 02:00","2021-04-04 01:30","2021-04-04 02:30","2021-10-03 02:15","2021-10-03 02:30","2021-10-31 02:30 CEST","2021-10-31 02:30 CET","2021-11-07 01:30 EST","2021-11-07 01:30 EDT","2021-06-15 12:00 Europe/London","1636263000","2021-06-15","Mon 2021-06-15","2021-03-28 01:30","2021-03-28 02:00"], $z);
}
// misc errors
add(["!Y-m-d", "!d", "!H:i", "!M", "!D", "!a", "!e", "!O", "!U", "!u", "!S", "!z", "!X", "!Y?m", "!Y,m", "!Y*m", "!jS"], ["", " ", "x", "\x80", "\xc3\x84", "\t", "-", "+"]);
add(["!Y-m-d H:i:s"], [" 2021-06-15 12:00:00", "2021-06-15  12:00:00", "2021-06-15 12:00:00 ", "2021-06-15\t12:00:00", "2021-06-1512:00:00"]);
add(["!d M Y", "!d F Y", "!j M Y"], [" 1 Jan 2021", "1 jan 2021", "01 Janvier 2021", "1 Ja 2021", "1  Jan 2021", "1 Jan  2021", "1Jan2021", "1 Jan2021"]);
add(["!d", "!j", "!m", "!n", "!H", "!G", "!h", "!g", "!i", "!s", "!Y", "!y", "!z", "!U", "!u", "!v", "!X", "!x"], ["\t5", " 5", "05x", "+5", "-5", "5", "005", " 05", "\n5"]);
add(["", "!", "|", "+"], ["", "x", " "]);
add(["!Y-m-d\\TH:i:sP", "!Y-m-d\\TH:i:s.uP", "!Y-m-d H:i:s.v e"], ["2021-06-15T10:30:45+02:00", "2021-06-15T10:30:45.123456-05:30", "2021-06-15 10:30:45.123 Europe/Paris", "2021-06-15T10:30:45Z", "2021-06-15 10:30:45.1 UTC"]);

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
DateTime::createFromFormat('Y', 'x'); $show('after failing cff');
DateTime::createFromFormat('Y', 'x'); date_parse_from_format('Y', '2021'); $show('after ok dpff (prev failing cff)');
DateTime::createFromFormat('Y', 'x'); date_parse('xx'); $show('after date_parse');
DateTime::createFromFormat('Y', 'x'); strtotime('xx'); $show('after strtotime');
DateTime::createFromFormat('Y', 'x'); new DateTime('now'); $show('after new DateTime now');
DateTime::createFromFormat('Y', 'x'); date_create('xx'); $show('after date_create fail');
DateTime::createFromFormat('Y', 'x'); date_create('2021-02-30'); $show('after date_create 02-30');
DateTime::createFromFormat('Y', 'x'); try { new DateTime('xx'); } catch (Throwable $e) {} $show('after new DateTime throw');
DateTime::createFromFormat('Y', 'x'); $d = new DateTime(); $show('after new DateTime()');
DateTime::createFromFormat('Y', 'x'); @$d->modify('xx'); $show('after modify fail');
DateTime::createFromFormat('Y', 'x'); $d->modify('+1 day'); $show('after modify ok');
DateTime::createFromFormat('Y', 'x'); DateTime::createFromFormat('Y', '2021'); $show('after ok cff');
DateTime::createFromFormat('Y+', '2021x'); $show('after warn cff');
DateTime::createFromFormat('Y', 'x'); DateTimeImmutable::createFromFormat('Y', '2021'); $show('after ok immutable cff');
DateTime::createFromFormat('Y', 'x'); echo "#\t", json_encode(DateTimeImmutable::getLastErrors()), "\timmutable getLastErrors\n";
DateTime::createFromFormat('Y', 'x'); new DateTimeZone('UTC'); $show('after DateTimeZone');
DateTime::createFromFormat('Y', 'x'); date_create_from_format('Y', '2021'); $show('after date_create_from_format ok');
DateTime::createFromFormat('Y', 'x'); $i = new DateTimeImmutable(); $i->modify('+1 day'); $show('after immutable modify ok');
DateTime::createFromFormat('Y', 'x'); $i->setDate(2021, 2, 30); $show('after setDate');
DateTime::createFromFormat('Y', 'x'); date_parse_from_format('Y', 'x'); $show('after dpff fail');
DateTime::createFromFormat('Y', 'x'); new DatePeriod('R2/2012-07-01T00:00:00Z/P7D'); $show('after DatePeriod iso');
DateTime::createFromFormat('Y', 'x'); $show('cff fail again: errors array shape');
try { DateTime::createFromFormat('Y', "20\0"); } catch (Throwable $e) { echo "#\t", get_class($e), ': ', $e->getMessage(), "\n"; }
try { DateTime::createFromFormat("Y\0", "20"); } catch (Throwable $e) { echo "#\t", get_class($e), ': ', $e->getMessage(), "\n"; }
try { date_create_from_format("Y\0", "20"); } catch (Throwable $e) { echo "#\t", get_class($e), ': ', $e->getMessage(), "\n"; }
try { date_parse_from_format("Y\0", "20"); } catch (Throwable $e) { echo "#\t", get_class($e), ': ', $e->getMessage(), "\n"; }
echo "#\t", timezone_version_get(), "\n";
