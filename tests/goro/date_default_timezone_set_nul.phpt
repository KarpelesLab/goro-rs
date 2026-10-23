--TEST--
date_default_timezone_set() treats the zone as a C string
--FILE--
<?php
var_dump(date_default_timezone_set("UTC\0x"), date_default_timezone_get());
var_dump(date_default_timezone_set("Europe/Paris\0x"), date_default_timezone_get());
var_dump(date_default_timezone_set("Bogus\0x"));
--EXPECTF--
bool(true)
string(3) "UTC"
bool(true)
string(12) "Europe/Paris"

Notice: date_default_timezone_set(): Timezone ID 'Bogus' is invalid in %s on line 4
bool(false)
