--TEST--
Class constant / static property fetches are on the member name's line
--FILE--
<?php
class K { const Y = 1; }
function t($f) { try { $f(); } catch (Error $e) { echo $e->getLine(), "\n"; } }
t(function() { return K
::Z; });
t(function() { return K::
Z; });
t(function() { return 1 + K
::Z; });
t(function() { return K
::Z + 1; });
t(function() { return [K
::Z]; });
t(function() { $k = 'K'; return $k
::Z; });
t(function() { return K
::$z; });
t(function() { return K
::m(); });
t(function() { return \K
::Z; });
t(function() { return static
::Z; });
--EXPECT--
5
7
9
11
13
15
17
19
21
23
