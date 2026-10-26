--TEST--
ini_get_all(): core entries, NULL defaults, access levels, case-insensitive order
--FILE--
<?php
var_dump(ini_get_all("standard")["user_agent"]);
ini_set("user_agent", "x");
var_dump(ini_get_all("standard")["user_agent"]);
ini_restore("user_agent");
var_dump(ini_get_all("standard", false)["user_agent"]);
var_dump(ini_get("user_agent"));
$all = ini_get_all();
var_dump($all["error_log"], $all["allow_url_fopen"]["access"], $all["memory_limit"]["access"]);
var_dump(count(ini_get_all("core")) === count($all));
$k = array_keys($all);
$i = array_search("SMTP", $k);
var_dump($k[$i - 1], $k[$i + 1]);
var_dump(ini_get_all("session")["session.name"]);
--EXPECT--
array(3) {
  ["global_value"]=>
  NULL
  ["local_value"]=>
  NULL
  ["access"]=>
  int(7)
}
array(3) {
  ["global_value"]=>
  string(1) "x"
  ["local_value"]=>
  string(1) "x"
  ["access"]=>
  int(7)
}
NULL
string(0) ""
array(3) {
  ["global_value"]=>
  NULL
  ["local_value"]=>
  NULL
  ["access"]=>
  int(7)
}
int(4)
int(7)
bool(true)
string(14) "short_open_tag"
string(9) "smtp_port"
array(3) {
  ["global_value"]=>
  string(9) "PHPSESSID"
  ["local_value"]=>
  string(9) "PHPSESSID"
  ["access"]=>
  int(7)
}
