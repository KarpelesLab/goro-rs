--TEST--
hash_file()/hash_hmac_file()/hash_update_file(): a failed read returns false; empty path
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
// A failed read makes the file functions return false
$dir = __DIR__;
t(fn() => hash_file("md5", $dir));
t(fn() => hash_hmac_file("md5", $dir, "k"));
t(fn() => md5_file($dir));
t(fn() => sha1_file($dir));
t(fn() => hash_update_file(hash_init("md5"), $dir));
t(fn() => hash_update_stream(hash_init("md5"), fopen($dir, "r")));
// an empty path
t(fn() => hash_file("md5", ""));
t(fn() => hash_hmac_file("md5", "", "k"));
t(fn() => md5_file(""));
t(fn() => hash_file("md5", "a\0b"));
t(fn() => hash_hmac_file("md5", "a\0b", "k"));
--EXPECTF--

Notice: hash_file(): Read of 8192 bytes failed with errno=21 Is a directory in %s on line 5
bool(false)

Notice: hash_hmac_file(): Read of 8192 bytes failed with errno=21 Is a directory in %s on line 6
bool(false)

Notice: md5_file(): Read of 8192 bytes failed with errno=21 Is a directory in %s on line 7
string(32) "d41d8cd98f00b204e9800998ecf8427e"

Notice: sha1_file(): Read of 8192 bytes failed with errno=21 Is a directory in %s on line 8
string(40) "da39a3ee5e6b4b0d3255bfef95601890afd80709"

Notice: hash_update_file(): Read of 8192 bytes failed with errno=21 Is a directory in %s on line 9
bool(false)

Notice: hash_update_stream(): Read of 8192 bytes failed with errno=21 Is a directory in %s on line 10
int(0)
ValueError: Path must not be empty
ValueError: Path must not be empty
ValueError: Path must not be empty
ValueError: hash_file(): Argument #1 ($algo) must not contain any null bytes
ValueError: hash_hmac_file(): Argument #2 ($filename) must not contain any null bytes
