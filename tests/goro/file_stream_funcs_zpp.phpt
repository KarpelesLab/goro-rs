--TEST--
fwrite()/fseek()/fread()/fgets()... parse their typed parameters after the stream; opendir()/dir()/scandir()/file() contexts and paths; flock() of a memory stream
--FILE--
<?php
$m = fopen("php://memory", "w+");
$closed = fopen("php://memory", "r"); fclose($closed);
foreach ([[$m, []], [$m, new stdClass], [$closed, []]] as [$a, $b]) {
    foreach (['fwrite', 'fputs', 'fseek', 'fread', 'fgets'] as $fn) {
        try { $fn($a, $b); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
    }
}
var_dump(fwrite($m, 12), fwrite($m, "abc", "2"), fwrite($m, null));
var_dump(fseek($m, "0"), fread($m, "3"));
try { fwrite($m, "x", "abc"); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { fclose(); } catch (ArgumentCountError $e) { echo $e->getMessage(), "\n"; }
try { rewind($m, 1); } catch (ArgumentCountError $e) { echo $e->getMessage(), "\n"; }
foreach (['opendir', 'dir', 'scandir'] as $fn) {
    try { $fn(".", ...($fn == 'scandir' ? [0, []] : [[]])); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
    try { $fn("a\0b"); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
}
var_dump(opendir(""));
try { file(__FILE__, 0, "ctx"); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { stream_filter_append($m, []); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { stream_filter_prepend($closed, "string.rot13"); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump(@stream_filter_append($m, "no\0such"), error_get_last()["message"]);
foreach (['parse_ini_file', 'stream_resolve_include_path'] as $fn) {
    try { $fn("a\0b"); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
}
try { clearstatcache(true, "a\0b"); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
try { move_uploaded_file("a\0b", "c"); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
try { socket_get_status(1); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump(flock($m, LOCK_EX, $wb), $wb);
var_dump(dir(__FILE__));
?>
--EXPECTF--
fwrite(): Argument #2 ($data) must be of type string, array given
fputs(): Argument #2 ($data) must be of type string, array given
fseek(): Argument #2 ($offset) must be of type int, array given
fread(): Argument #2 ($length) must be of type int, array given
fgets(): Argument #2 ($length) must be of type ?int, array given
fwrite(): Argument #2 ($data) must be of type string, stdClass given
fputs(): Argument #2 ($data) must be of type string, stdClass given
fseek(): Argument #2 ($offset) must be of type int, stdClass given
fread(): Argument #2 ($length) must be of type int, stdClass given
fgets(): Argument #2 ($length) must be of type ?int, stdClass given
fwrite(): Argument #1 ($stream) must be an open stream resource
fputs(): Argument #1 ($stream) must be an open stream resource
fseek(): Argument #1 ($stream) must be an open stream resource
fread(): Argument #1 ($stream) must be an open stream resource
fgets(): Argument #1 ($stream) must be an open stream resource

Deprecated: fwrite(): Passing null to parameter #2 ($data) of type string is deprecated in %s on line %d
int(2)
int(2)
int(0)
int(0)
string(3) "12a"
fwrite(): Argument #3 ($length) must be of type ?int, string given
fclose() expects exactly 1 argument, 0 given
rewind() expects exactly 1 argument, 2 given
opendir(): Argument #2 ($context) must be of type resource or null, array given
opendir(): Argument #1 ($directory) must not contain any null bytes
dir(): Argument #2 ($context) must be of type resource or null, array given
dir(): Argument #1 ($directory) must not contain any null bytes
scandir(): Argument #3 ($context) must be of type resource or null, array given
scandir(): Argument #1 ($directory) must not contain any null bytes
bool(false)
file(): Argument #3 ($context) must be of type resource or null, string given
stream_filter_append(): Argument #2 ($filter_name) must be of type string, array given
stream_filter_prepend(): Argument #1 ($stream) must be an open stream resource
bool(false)
string(%d) "stream_filter_append(): Unable to locate filter "no""
parse_ini_file(): Argument #1 ($filename) must not contain any null bytes
stream_resolve_include_path(): Argument #1 ($filename) must not contain any null bytes
clearstatcache(): Argument #2 ($filename) must not contain any null bytes
move_uploaded_file(): Argument #1 ($from) must not contain any null bytes
socket_get_status(): Argument #1 ($stream) must be of type resource, int given
bool(false)
int(0)

Warning: dir(%s): Failed to open directory: Not a directory in %s on line %d
bool(false)
