--TEST--
gzopen() of php://temp (php_stream_temp_cast() moves it to a temporary file)
--FILE--
<?php
foreach (['rb9', 'wb2', 'wb6f', 'ab9'] as $mode) {
    $r = gzopen('php://temp', $mode);
    var_dump(is_resource($r), stream_get_meta_data($r)['mode']);
    if ($mode[0] !== 'r') {
        var_dump(gzwrite($r, "hello world"));
    } else {
        var_dump(gzread($r, 10));
    }
    var_dump(gzclose($r));
}
$t = fopen('php://temp', 'w+');
fwrite($t, "abc");
$g = gzopen('php://memory', 'rb');
var_dump($g);

?>
--EXPECTF--
bool(true)
string(3) "rb9"
string(0) ""
bool(true)
bool(true)
string(3) "wb2"
int(11)
bool(true)
bool(true)
string(4) "wb6f"
int(11)
bool(true)
bool(true)
string(3) "ab9"
int(11)
bool(true)

Warning: gzopen(): Cannot represent a stream of type MEMORY as a File Descriptor in %s on line %d
bool(false)
