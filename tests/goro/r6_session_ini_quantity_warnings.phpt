--TEST--
session INI integers warn about malformed quantities
--FILE--
<?php
ob_start();
foreach (['gc_probability', 'gc_divisor', 'gc_maxlifetime', 'cache_expire', 'cookie_lifetime'] as $k) {
    foreach (['abc', '1.5', '2k', '-3', '99999999999999999999'] as $v) {
        echo "$k=$v: ";
        var_dump(ini_set("session.$k", $v), ini_get("session.$k"));
    }
}
--EXPECTF--
gc_probability=abc: 
Warning: Invalid "session.gc_probability" setting. Invalid quantity "abc": no valid leading digits, interpreting as "0" for backwards compatibility in %s on line 6
string(1) "1"
string(3) "abc"
gc_probability=1.5: 
Warning: Invalid "session.gc_probability" setting. Invalid quantity "1.5": unknown multiplier "5", interpreting as "1" for backwards compatibility in %s on line 6
string(3) "abc"
string(3) "1.5"
gc_probability=2k: string(3) "1.5"
string(2) "2k"
gc_probability=-3: 
Warning: ini_set(): session.gc_probability must be greater than or equal to 0 in %s on line 6
bool(false)
string(2) "2k"
gc_probability=99999999999999999999: 
Warning: Invalid "session.gc_probability" setting. Invalid quantity "99999999999999999999": value is out of range, using overflow result for backwards compatibility in %s on line 6

Warning: ini_set(): session.gc_probability must be greater than or equal to 0 in %s on line 6
bool(false)
string(2) "2k"
gc_divisor=abc: 
Warning: Invalid "session.gc_divisor" setting. Invalid quantity "abc": no valid leading digits, interpreting as "0" for backwards compatibility in %s on line 6

Warning: ini_set(): session.gc_divisor must be greater than 0 in %s on line 6
bool(false)
string(3) "100"
gc_divisor=1.5: 
Warning: Invalid "session.gc_divisor" setting. Invalid quantity "1.5": unknown multiplier "5", interpreting as "1" for backwards compatibility in %s on line 6
string(3) "100"
string(3) "1.5"
gc_divisor=2k: string(3) "1.5"
string(2) "2k"
gc_divisor=-3: 
Warning: ini_set(): session.gc_divisor must be greater than 0 in %s on line 6
bool(false)
string(2) "2k"
gc_divisor=99999999999999999999: 
Warning: Invalid "session.gc_divisor" setting. Invalid quantity "99999999999999999999": value is out of range, using overflow result for backwards compatibility in %s on line 6

Warning: ini_set(): session.gc_divisor must be greater than 0 in %s on line 6
bool(false)
string(2) "2k"
gc_maxlifetime=abc: 
Warning: Invalid "session.gc_maxlifetime" setting. Invalid quantity "abc": no valid leading digits, interpreting as "0" for backwards compatibility in %s on line 6
string(4) "1440"
string(3) "abc"
gc_maxlifetime=1.5: 
Warning: Invalid "session.gc_maxlifetime" setting. Invalid quantity "1.5": unknown multiplier "5", interpreting as "1" for backwards compatibility in %s on line 6
string(3) "abc"
string(3) "1.5"
gc_maxlifetime=2k: string(3) "1.5"
string(2) "2k"
gc_maxlifetime=-3: string(2) "2k"
string(2) "-3"
gc_maxlifetime=99999999999999999999: 
Warning: Invalid "session.gc_maxlifetime" setting. Invalid quantity "99999999999999999999": value is out of range, using overflow result for backwards compatibility in %s on line 6
string(2) "-3"
string(20) "99999999999999999999"
cache_expire=abc: 
Warning: Invalid "session.cache_expire" setting. Invalid quantity "abc": no valid leading digits, interpreting as "0" for backwards compatibility in %s on line 6
string(3) "180"
string(3) "abc"
cache_expire=1.5: 
Warning: Invalid "session.cache_expire" setting. Invalid quantity "1.5": unknown multiplier "5", interpreting as "1" for backwards compatibility in %s on line 6
string(3) "abc"
string(3) "1.5"
cache_expire=2k: string(3) "1.5"
string(2) "2k"
cache_expire=-3: string(2) "2k"
string(2) "-3"
cache_expire=99999999999999999999: 
Warning: Invalid "session.cache_expire" setting. Invalid quantity "99999999999999999999": value is out of range, using overflow result for backwards compatibility in %s on line 6
string(2) "-3"
string(20) "99999999999999999999"
cookie_lifetime=abc: 
Warning: Invalid "session.cookie_lifetime" setting. Invalid quantity "abc": no valid leading digits, interpreting as "0" for backwards compatibility in %s on line 6
string(1) "0"
string(3) "abc"
cookie_lifetime=1.5: 
Warning: Invalid "session.cookie_lifetime" setting. Invalid quantity "1.5": unknown multiplier "5", interpreting as "1" for backwards compatibility in %s on line 6
string(3) "abc"
string(3) "1.5"
cookie_lifetime=2k: string(3) "1.5"
string(2) "2k"
cookie_lifetime=-3: 
Warning: ini_set(): CookieLifetime cannot be negative in %s on line 6
bool(false)
string(2) "2k"
cookie_lifetime=99999999999999999999: string(2) "2k"
string(20) "99999999999999999999"
