--TEST--
disk_*_space("") is false, getimagesize("") "Path must not be empty", http_response_code(0) reads, pos() deprecation name, get_resources() unknown type
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => var_dump(disk_total_space("")));
t(fn() => var_dump(disk_free_space("")));
t(fn() => var_dump(diskfreespace("")));
t(fn() => var_dump(getimagesize("")));
t(fn() => var_dump(http_response_code(0)));
t(fn() => var_dump(http_response_code(null)));
t(fn() => var_dump(pos(new ArrayObject([1]))));
t(fn() => var_dump(get_resources("abc")));
t(fn() => var_dump(count(get_resources("stream")) > 0));
t(fn() => var_dump(get_resources("Unknown")));
t(fn() => var_dump(get_resources("stream-context")));
t(fn() => var_dump(get_resources("")));
--EXPECTF--
bool(false)
bool(false)
bool(false)
ValueError: Path must not be empty
bool(false)

Deprecated: http_response_code(): Passing null to parameter #1 ($response_code) of type int is deprecated in %s on line 8
bool(false)

Deprecated: pos(): Calling pos() on an object is deprecated in %s on line 9
bool(false)
ValueError: get_resources(): Argument #1 ($type) must be a valid resource type
bool(true)
array(0) {
}
array(0) {
}
ValueError: get_resources(): Argument #1 ($type) must be a valid resource type
