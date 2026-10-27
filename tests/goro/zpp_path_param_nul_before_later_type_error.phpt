--TEST--
A Z_PARAM_PATH argument with a NUL byte is zpp's ValueError before a later argument's TypeError
--FILE--
<?php
function t($c) { try { var_dump($c()); } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; } }
t(fn() => fnmatch("a\0b", "x", "abc"));
t(fn() => fnmatch("a", "x\0", "abc"));
t(fn() => fnmatch("a", "x\0"));
t(fn() => fnmatch([], "x\0"));
t(fn() => fnmatch("a\0", []));
t(fn() => date_create_from_format("Y", "a\0b", "UTC"));
t(fn() => date_create_immutable_from_format("Y", "a\0b", []));
t(fn() => DateTime::createFromFormat("Y", "a\0b", "UTC"));
t(fn() => date_parse_from_format("Y", "a\0b"));
t(fn() => file_get_contents("a\0b", "x"));
t(fn() => file_get_contents("a\0b", false, null, "x"));
t(fn() => file_put_contents("a\0b", "x", "y"));
t(fn() => md5_file("a\0b", []));
t(fn() => sha1_file("a\0b", "x"));
t(fn() => mkdir("a\0b", "x"));
t(fn() => touch("a\0b", "x"));
t(fn() => copy("a\0b", "c", "x"));
t(fn() => rename("a", "c\0", "x"));
t(fn() => tempnam("a\0", []));
t(fn() => glob("a\0", "x"));
t(fn() => scandir("a\0", "x"));
t(fn() => parse_ini_file("a\0", "x"));
t(fn() => readfile("a\0", "x"));
t(fn() => file("a\0", "x"));
t(fn() => fopen("a\0", "r", "x"));
t(fn() => hash_update_file(hash_init('md5'), "a\0", "x"));
t(fn() => clearstatcache("x", "a\0"));
t(fn() => DateTimeImmutable::createFromFormat("Y", "a\0b", 5));
t(fn() => new SplFileObject("a\0b", []));
--EXPECT--
ValueError: fnmatch(): Argument #1 ($pattern) must not contain any null bytes
ValueError: fnmatch(): Argument #2 ($filename) must not contain any null bytes
ValueError: fnmatch(): Argument #2 ($filename) must not contain any null bytes
TypeError: fnmatch(): Argument #1 ($pattern) must be of type string, array given
ValueError: fnmatch(): Argument #1 ($pattern) must not contain any null bytes
ValueError: date_create_from_format(): Argument #2 ($datetime) must not contain any null bytes
ValueError: date_create_immutable_from_format(): Argument #2 ($datetime) must not contain any null bytes
ValueError: DateTime::createFromFormat(): Argument #2 ($datetime) must not contain any null bytes
ValueError: date_parse_from_format(): Argument #2 ($datetime) must not contain any null bytes
ValueError: file_get_contents(): Argument #1 ($filename) must not contain any null bytes
ValueError: file_get_contents(): Argument #1 ($filename) must not contain any null bytes
ValueError: file_put_contents(): Argument #1 ($filename) must not contain any null bytes
ValueError: md5_file(): Argument #1 ($filename) must not contain any null bytes
ValueError: sha1_file(): Argument #1 ($filename) must not contain any null bytes
ValueError: mkdir(): Argument #1 ($directory) must not contain any null bytes
ValueError: touch(): Argument #1 ($filename) must not contain any null bytes
ValueError: copy(): Argument #1 ($from) must not contain any null bytes
ValueError: rename(): Argument #2 ($to) must not contain any null bytes
ValueError: tempnam(): Argument #1 ($directory) must not contain any null bytes
ValueError: glob(): Argument #1 ($pattern) must not contain any null bytes
ValueError: scandir(): Argument #1 ($directory) must not contain any null bytes
ValueError: parse_ini_file(): Argument #1 ($filename) must not contain any null bytes
ValueError: readfile(): Argument #1 ($filename) must not contain any null bytes
ValueError: file(): Argument #1 ($filename) must not contain any null bytes
ValueError: fopen(): Argument #1 ($filename) must not contain any null bytes
ValueError: hash_update_file(): Argument #2 ($filename) must not contain any null bytes
ValueError: clearstatcache(): Argument #2 ($filename) must not contain any null bytes
ValueError: DateTimeImmutable::createFromFormat(): Argument #2 ($datetime) must not contain any null bytes
ValueError: SplFileObject::__construct(): Argument #1 ($filename) must not contain any null bytes
