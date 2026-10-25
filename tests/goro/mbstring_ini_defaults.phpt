--TEST--
mbstring INI entries: defaults and ini_get_all()
--FILE--
<?php
foreach (["mbstring.language", "mbstring.strict_detection", "mbstring.encoding_translation", "mbstring.substitute_character", "mbstring.detect_order", "mbstring.http_output_conv_mimetypes", "mbstring.func_overload"] as $k) {
    var_dump(ini_get($k));
}
var_dump(ini_set("mbstring.language", "Japanese"), mb_language());
var_dump(ini_set("mbstring.strict_detection", "1"));
var_dump(ini_set("mbstring.encoding_translation", "1"));
$all = ini_get_all("mbstring");
var_dump($all["mbstring.language"], $all["mbstring.encoding_translation"]["access"]);
--EXPECT--
string(7) "neutral"
string(1) "0"
string(1) "0"
string(0) ""
string(0) ""
string(31) "^(text/|application/xhtml\+xml)"
bool(false)
string(7) "neutral"
string(8) "Japanese"
string(1) "0"
bool(false)
array(3) {
  ["global_value"]=>
  string(7) "neutral"
  ["local_value"]=>
  string(8) "Japanese"
  ["access"]=>
  int(7)
}
int(6)
