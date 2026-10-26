--TEST--
filestat.c FileFunction()s parse a string filename (type errors, null deprecation, __toString); NUL bytes in paths
--FILE--
<?php
class S { function __toString(): string { return "."; } }
foreach (['filesize', 'is_dir', 'file_exists', 'stat', 'filemtime', 'is_writable'] as $fn) {
    foreach (['null' => null, 'arr' => [], 'obj' => new stdClass, 'S' => new S] as $k => $v) {
        try { $r = $fn($v); echo "$fn($k): ", is_array($r) ? "array" : gettype($r), "\n"; }
        catch (TypeError $e) { echo "$fn($k): ", $e->getMessage(), "\n"; }
    }
    try { $fn(); } catch (ArgumentCountError $e) { echo $e->getMessage(), "\n"; }
}
foreach (['filesize', 'filetype', 'stat', 'lstat', 'fileatime', 'filemtime', 'filectime', 'is_file', 'file_exists'] as $fn) {
    var_dump($fn("a\0b"));
}
foreach (['realpath', 'readlink', 'linkinfo', 'is_uploaded_file'] as $fn) {
    try { $fn("a\0b"); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
}
var_dump(disk_free_space(""), disk_total_space(""));
var_dump(stream_resolve_include_path("") === getcwd());
?>
--EXPECTF--
Deprecated: filesize(): Passing null to parameter #1 ($filename) of type string is deprecated in %s on line %d
filesize(null): boolean
filesize(arr): filesize(): Argument #1 ($filename) must be of type string, array given
filesize(obj): filesize(): Argument #1 ($filename) must be of type string, stdClass given
filesize(S): integer
filesize() expects exactly 1 argument, 0 given

Deprecated: is_dir(): Passing null to parameter #1 ($filename) of type string is deprecated in %s on line %d
is_dir(null): boolean
is_dir(arr): is_dir(): Argument #1 ($filename) must be of type string, array given
is_dir(obj): is_dir(): Argument #1 ($filename) must be of type string, stdClass given
is_dir(S): boolean
is_dir() expects exactly 1 argument, 0 given

Deprecated: file_exists(): Passing null to parameter #1 ($filename) of type string is deprecated in %s on line %d
file_exists(null): boolean
file_exists(arr): file_exists(): Argument #1 ($filename) must be of type string, array given
file_exists(obj): file_exists(): Argument #1 ($filename) must be of type string, stdClass given
file_exists(S): boolean
file_exists() expects exactly 1 argument, 0 given

Deprecated: stat(): Passing null to parameter #1 ($filename) of type string is deprecated in %s on line %d
stat(null): boolean
stat(arr): stat(): Argument #1 ($filename) must be of type string, array given
stat(obj): stat(): Argument #1 ($filename) must be of type string, stdClass given
stat(S): array
stat() expects exactly 1 argument, 0 given

Deprecated: filemtime(): Passing null to parameter #1 ($filename) of type string is deprecated in %s on line %d
filemtime(null): boolean
filemtime(arr): filemtime(): Argument #1 ($filename) must be of type string, array given
filemtime(obj): filemtime(): Argument #1 ($filename) must be of type string, stdClass given
filemtime(S): integer
filemtime() expects exactly 1 argument, 0 given

Deprecated: is_writable(): Passing null to parameter #1 ($filename) of type string is deprecated in %s on line %d
is_writable(null): boolean
is_writable(arr): is_writable(): Argument #1 ($filename) must be of type string, array given
is_writable(obj): is_writable(): Argument #1 ($filename) must be of type string, stdClass given
is_writable(S): boolean
is_writable() expects exactly 1 argument, 0 given

Warning: filesize(): Filename contains null byte in %s on line %d
bool(false)

Warning: filetype(): Filename contains null byte in %s on line %d
bool(false)

Warning: stat(): Filename contains null byte in %s on line %d
bool(false)

Warning: lstat(): Filename contains null byte in %s on line %d
bool(false)

Warning: fileatime(): Filename contains null byte in %s on line %d
bool(false)

Warning: filemtime(): Filename contains null byte in %s on line %d
bool(false)

Warning: filectime(): Filename contains null byte in %s on line %d
bool(false)
bool(false)
bool(false)
realpath(): Argument #1 ($path) must not contain any null bytes
readlink(): Argument #1 ($path) must not contain any null bytes
linkinfo(): Argument #1 ($path) must not contain any null bytes
is_uploaded_file(): Argument #1 ($filename) must not contain any null bytes
bool(false)
bool(false)
bool(true)
