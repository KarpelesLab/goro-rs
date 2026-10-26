--TEST--
ini_set(): pcre limits quantity warnings, string_param_max_len overflow, mbstring.language '', ini bool strtol
--FILE--
<?php
foreach (["pcre.backtrack_limit" => ["abc", "1.5", "99999999999999999999", "10K"], "pcre.recursion_limit" => ["x"], "zend.exception_string_param_max_len" => ["99999999999999999999", "-1", "7"], "mbstring.language" => ["", "uni"], "auto_detect_line_endings" => ["99999999999999999999"], "assert.active" => ["99999999999999999999"]] as $n => $vs) {
    foreach ($vs as $v) {
        var_dump(ini_set($n, $v), ini_get($n));
    }
}
--EXPECTF--

Warning: Invalid "pcre.backtrack_limit" setting. Invalid quantity "abc": no valid leading digits, interpreting as "0" for backwards compatibility in %s on line 4
string(7) "1000000"
string(3) "abc"

Warning: Invalid "pcre.backtrack_limit" setting. Invalid quantity "1.5": unknown multiplier "5", interpreting as "1" for backwards compatibility in %s on line 4
string(3) "abc"
string(3) "1.5"

Warning: Invalid "pcre.backtrack_limit" setting. Invalid quantity "99999999999999999999": value is out of range, using overflow result for backwards compatibility in %s on line 4
string(3) "1.5"
string(20) "99999999999999999999"
string(20) "99999999999999999999"
string(3) "10K"

Warning: Invalid "pcre.recursion_limit" setting. Invalid quantity "x": no valid leading digits, interpreting as "0" for backwards compatibility in %s on line 4
string(6) "100000"
string(1) "x"
bool(false)
string(2) "15"
bool(false)
string(2) "15"
string(2) "15"
string(1) "7"
bool(false)
string(7) "neutral"
string(7) "neutral"
string(3) "uni"

Deprecated: auto_detect_line_endings is deprecated in %s on line 4
string(1) "0"
string(20) "99999999999999999999"
string(1) "1"
string(20) "99999999999999999999"
