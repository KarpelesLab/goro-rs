--TEST--
Headers sent by a displayed request-startup error have no output position
--INI--
max_input_vars=1
display_errors=1
session.use_cookies=1
--GET--
a=1&b=2
--FILE--
<?php
var_dump(headers_sent($file, $line), $file, $line);
var_dump(session_start());
?>
--EXPECTF--
Warning: PHP Request Startup: Input variables exceeded 1. To increase the limit change max_input_vars in php.ini. in Unknown on line 0
bool(true)
string(0) ""
int(0)

Warning: session_start(): Session cannot be started after headers have already been sent in %s on line %d
bool(false)
