--TEST--
Output produced by a user output handler: the function the deprecation names
--FILE--
<?php
ob_start(function ($b) { echo "x"; return "[$b]"; }, 2);
echo "abc";
echo "\n";
function f() { echo "def"; }
f();
ob_end_flush();
ob_start(function ($b) { echo "y"; return "<$b>"; });
echo "tail";
--EXPECTF--

Deprecated: main(): Producing output from user output handler {closure:%s:2} is deprecated in %s on line 3
[abc]
Deprecated: f(): Producing output from user output handler {closure:%s:2} is deprecated in %s on line 5
[
def]
Deprecated: ob_end_flush(): Producing output from user output handler {closure:%s:2} is deprecated in %s on line 7
[]
Deprecated: PHP Request Shutdown: Producing output from user output handler {closure:%s:8} is deprecated in Unknown on line 0
<tail>
