--TEST--
include of a path with a NUL byte stops the message at the NUL; including a directory fails with ENOTTY
--FILE--
<?php
var_dump(include "a\0b");
var_dump(include_once "a\0b");
var_dump(include __DIR__);
var_dump((include_once "") === false);
try {
    require "a\0b";
} catch (Error $e) {
    echo $e->getMessage(), "\n";
}
--EXPECTF--
Warning: include(): Failed opening 'a' for inclusion (include_path='%s') in %s on line 2
bool(false)

Warning: include_once(): Failed opening 'a' for inclusion (include_path='%s') in %s on line 3
bool(false)

Warning: include(%s): Failed to open stream: Inappropriate ioctl for device in %s on line 4

Warning: include(): Failed opening '%s' for inclusion (include_path='%s') in %s on line 4
bool(false)

Warning: include_once(%s): Failed to open stream: Inappropriate ioctl for device in %s on line 5

Warning: include_once(): Failed opening '' for inclusion (include_path='%s') in %s on line 5
bool(true)
Failed opening required 'a' (include_path='%s')
