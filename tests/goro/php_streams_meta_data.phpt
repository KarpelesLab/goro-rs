--TEST--
stream_get_meta_data() of php://temp, php://output, php://input, php://filter, glob:// and php://fd; invalid php:// URLs
--FILE--
<?php
function md($f) {
    $m = stream_get_meta_data($f);
    unset($m['uri']);
    echo json_encode($m), "\n";
}
md(fopen('php://temp', 'r'));
md(fopen('php://temp/maxmemory:4', 'w+'));
md(fopen('php://output', 'r'));
md(fopen('php://input', 'w'));
$tmp = tempnam(sys_get_temp_dir(), 'md');
file_put_contents($tmp, "abc");
$f = fopen("php://filter/read=string.rot13/resource=$tmp", 'r');
md($f);
var_dump(stream_get_meta_data($f)['uri'] === "php://filter/read=string.rot13/resource=$tmp");
md(fopen('php://filter/read=string.toupper/resource=data:,abc', 'r'));
$d = opendir('glob://' . dirname($tmp) . '/' . basename($tmp));
md($d);
var_dump(readdir($d) === basename($tmp), readdir($d));
rewinddir($d);
var_dump(readdir($d) === basename($tmp));
$p = fopen($tmp, 'r');
$fd = fopen('php://fd/' . (PHP_OS_FAMILY === 'Linux' ? 0 : 0), 'r');
var_dump(is_bool(stream_get_meta_data($fd)['seekable']));
unlink($tmp);
var_dump(fopen('php://fd/abc', 'r'));
var_dump(fopen('php://fd/-1', 'r'));
var_dump(fopen('php://nope', 'r'));
var_dump(fopen('php://filter', 'r'));
$f = fopen('data:,abc', 'r');
var_dump(fwrite($f, 'x'));
?>
--EXPECTF--
{"wrapper_type":"PHP","stream_type":"TEMP","mode":"rb","unread_bytes":0,"seekable":true}
{"wrapper_type":"PHP","stream_type":"TEMP","mode":"w+b","unread_bytes":0,"seekable":true}
{"timed_out":false,"blocked":true,"eof":false,"wrapper_type":"PHP","stream_type":"Output","mode":"wb","unread_bytes":0,"seekable":false}
{"timed_out":false,"blocked":true,"eof":false,"wrapper_type":"PHP","stream_type":"Input","mode":"rb","unread_bytes":0,"seekable":true}
{"timed_out":false,"blocked":true,"eof":false,"wrapper_type":"PHP","stream_type":"STDIO","mode":"r","unread_bytes":0,"seekable":true}
bool(true)
{"base64":false,"wrapper_type":"PHP","stream_type":"RFC2397","mode":"r","unread_bytes":0,"seekable":true}
{"timed_out":false,"blocked":true,"eof":false,"wrapper_type":"glob","stream_type":"glob","mode":"r","unread_bytes":0,"seekable":true}
bool(true)
bool(false)
bool(true)
bool(true)

Warning: fopen(php://fd/abc): Failed to open stream: php://fd/ stream must be specified in the form php://fd/<orig fd> in %s on line %d
bool(false)

Warning: fopen(php://fd/-1): Failed to open stream: The file descriptors must be non-negative numbers smaller than %d in %s on line %d
bool(false)

Warning: fopen(): Invalid php:// URL specified in %s on line %d

Warning: fopen(php://nope): Failed to open stream: operation failed in %s on line %d
bool(false)

Warning: fopen(): Invalid php:// URL specified in %s on line %d

Warning: fopen(php://filter): Failed to open stream: operation failed in %s on line %d
bool(false)

Notice: fwrite(): Stream is not writable in %s on line %d
bool(false)
