--TEST--
php://memory and php://temp opened for appending: writes go to the end, the position counts the bytes written
--FILE--
<?php
foreach (["php://memory", "php://temp"] as $u) {
    $a = fopen($u, "a");
    var_dump(ftell($a), fwrite($a, "abc"), ftell($a), fseek($a, 1), ftell($a), fread($a, 1), ftell($a), fwrite($a, "Z"), ftell($a));
    var_dump(fseek($a, 0, SEEK_END), ftell($a), fwrite($a, "Q"), ftell($a));
    rewind($a);
    var_dump(stream_get_contents($a));
}
?>
--EXPECT--
int(0)
int(3)
int(3)
int(0)
int(1)
string(1) "b"
int(2)
int(1)
int(3)
int(0)
int(4)
int(1)
int(5)
string(5) "abcZQ"
int(0)
int(3)
int(3)
int(0)
int(1)
string(1) "b"
int(2)
int(1)
int(3)
int(0)
int(4)
int(1)
int(5)
string(5) "abcZQ"
