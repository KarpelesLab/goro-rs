--TEST--
session_encode() warns about numeric keys as it meets them; session_decode()'s normalization pass warns too (not php_binary's early failure)
--FILE--
<?php
ob_start();
ini_set('session.save_path', sys_get_temp_dir());
ini_set('session.use_cookies', 0);
ini_set('session.cache_limiter', '');
foreach (['php', 'php_binary', 'php_serialize'] as $h) {
  ini_set('session.serialize_handler', $h);
  session_id('goroNumKey' . getmypid() . str_replace('_', '', $h));
  session_start();
  echo "== $h\n";
  $_SESSION = ['a' => 1, 5 => 'num', 'c|d' => 2, 7 => 'x', 'b' => 2];
  var_dump(session_encode());
  unset($_SESSION['c|d']);
  var_dump(session_encode());
  var_dump(session_decode('x|i:1;'), count($_SESSION));
  var_dump(session_decode('broken'));
  session_destroy();
}
?>
--EXPECTF--
== php

Warning: session_encode(): Skipping numeric key 5 in %s on line 12

Warning: session_encode(): Failed to write session data. Data contains invalid key "c|d" in %s on line 12
bool(false)

Warning: session_encode(): Skipping numeric key 5 in %s on line 14

Warning: session_encode(): Skipping numeric key 7 in %s on line 14
string(12) "a|i:1;b|i:2;"

Warning: session_decode(): Skipping numeric key 5 in %s on line 15

Warning: session_decode(): Skipping numeric key 7 in %s on line 15
bool(true)
int(5)

Warning: session_decode(): Skipping numeric key 5 in %s on line 16

Warning: session_decode(): Skipping numeric key 7 in %s on line 16

Warning: session_decode(): Failed to decode session object. Session has been destroyed in %s on line 16
bool(false)

Warning: session_destroy(): Trying to destroy uninitialized session in %s on line 17
== php_binary

Warning: session_encode(): Skipping numeric key 5 in %s on line 12

Warning: session_encode(): Skipping numeric key 7 in %s on line 12
string(20) "ai:1;c|di:2;bi:2;"

Warning: session_encode(): Skipping numeric key 5 in %s on line 14

Warning: session_encode(): Skipping numeric key 7 in %s on line 14
string(12) "ai:1;bi:2;"

Warning: session_decode(): Failed to decode session object. Session has been destroyed in %s on line 15
bool(false)
int(0)

Warning: session_decode(): Session data cannot be decoded when there is no active session in %s on line 16
bool(false)

Warning: session_destroy(): Trying to destroy uninitialized session in %s on line 17
== php_serialize
string(70) "a:5:{s:1:"a";i:1;i:5;s:3:"num";s:3:"c|d";i:2;i:7;s:1:"x";s:1:"b";i:2;}"
string(56) "a:4:{s:1:"a";i:1;i:5;s:3:"num";i:7;s:1:"x";s:1:"b";i:2;}"

Warning: session_decode(): Failed to decode session object. Session has been destroyed in %s on line 15
bool(false)
int(0)

Warning: session_decode(): Session data cannot be decoded when there is no active session in %s on line 16
bool(false)

Warning: session_destroy(): Trying to destroy uninitialized session in %s on line 17
