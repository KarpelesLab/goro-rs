--TEST--
date.default_latitude and friends are registered INI entries
--FILE--
<?php
foreach (['date.default_latitude','date.default_longitude','date.sunrise_zenith','date.sunset_zenith'] as $k) var_dump(ini_get($k));
var_dump(ini_set('date.default_latitude', '10'), ini_get('date.default_latitude'));
var_dump(@date_sunrise(1700000000, SUNFUNCS_RET_STRING));
ini_set('date.sunrise_zenith', '96');
var_dump(@date_sunrise(1700000000, SUNFUNCS_RET_STRING));
--EXPECT--
string(7) "31.7667"
string(7) "35.2333"
string(9) "90.833333"
string(9) "90.833333"
string(7) "31.7667"
string(2) "10"
string(5) "03:32"
string(5) "03:09"
