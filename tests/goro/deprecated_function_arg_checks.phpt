--TEST--
utf8_encode()/utf8_decode()/strptime(): deprecation, then the argument checks
--FILE--
<?php
function f($x) { return utf8_decode($x); }
foreach (["abc", "\xc3\xa9", "\xe2\x82\xac", "\xff", "\xc3", "a\xc3(b", "\xf0\x9f\x98\x80", "\xed\xa0\x80", "\xc0\xaf", "\xe0\x80\xaf", "\xf4\x90\x80\x80"] as $s) echo bin2hex(@utf8_decode($s)), " ", bin2hex(@utf8_encode($s)), "\n";
foreach ([[], [[]], ["a", "b"], [null], [new stdClass]] as $args) {
    try { var_dump(utf8_encode(...$args)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { var_dump(strptime(...$args)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
var_dump((new ReflectionFunction('utf8_encode'))->isDeprecated(), (new ReflectionFunction('strlen'))->isDeprecated());
set_error_handler(function ($n, $s) { throw new Exception($s); });
try { utf8_encode([]); } catch (Exception $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
f(new stdClass);
--EXPECTF--
616263 616263
e9 c383c2a9
3f c3a2c282c2ac
3f c3bf
3f c383
613f2862 61c3832862
3f c3b0c29fc298c280
3f c3adc2a0c280
3f3f c380c2af
3f c3a0c280c2af
3f c3b4c290c280c280

Deprecated: Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives in %s on line 5
ArgumentCountError: utf8_encode() expects exactly 1 argument, 0 given

Deprecated: Function strptime() is deprecated since 8.2, use date_parse_from_format() (for locale-independent parsing), or IntlDateFormatter::parse() (for locale-dependent parsing) instead in %s on line 6
ArgumentCountError: strptime() expects exactly 2 arguments, 0 given

Deprecated: Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives in %s on line 5
TypeError: utf8_encode(): Argument #1 ($string) must be of type string, array given

Deprecated: Function strptime() is deprecated since 8.2, use date_parse_from_format() (for locale-independent parsing), or IntlDateFormatter::parse() (for locale-dependent parsing) instead in %s on line 6
ArgumentCountError: strptime() expects exactly 2 arguments, 1 given

Deprecated: Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives in %s on line 5
ArgumentCountError: utf8_encode() expects exactly 1 argument, 2 given

Deprecated: Function strptime() is deprecated since 8.2, use date_parse_from_format() (for locale-independent parsing), or IntlDateFormatter::parse() (for locale-dependent parsing) instead in %s on line 6
bool(false)

Deprecated: Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives in %s on line 5

Deprecated: utf8_encode(): Passing null to parameter #1 ($string) of type string is deprecated in %s on line 5
string(0) ""

Deprecated: Function strptime() is deprecated since 8.2, use date_parse_from_format() (for locale-independent parsing), or IntlDateFormatter::parse() (for locale-dependent parsing) instead in %s on line 6
ArgumentCountError: strptime() expects exactly 2 arguments, 1 given

Deprecated: Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives in %s on line 5
TypeError: utf8_encode(): Argument #1 ($string) must be of type string, stdClass given

Deprecated: Function strptime() is deprecated since 8.2, use date_parse_from_format() (for locale-independent parsing), or IntlDateFormatter::parse() (for locale-dependent parsing) instead in %s on line 6
ArgumentCountError: strptime() expects exactly 2 arguments, 1 given
bool(true)
bool(false)
Exception: Function utf8_encode() is deprecated since 8.2, visit the php.net documentation for various alternatives

Fatal error: Uncaught Exception: Function utf8_decode() is deprecated since 8.2, visit the php.net documentation for various alternatives in %s:9
Stack trace:
#0 %s(2): {closure:%s:9}(8192, 'Function utf8_d...', '/home/magicaltu...', 2)
#1 %s(11): f(Object(stdClass))
#2 {main}
  thrown in %s on line 9
