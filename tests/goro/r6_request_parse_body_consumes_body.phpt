--TEST--
request_parse_body() reads the SAPI body once: multipart leaves php://input empty
--INI--
enable_post_data_reading=0
--POST_RAW--
Content-Type: multipart/form-data; boundary=XyZ
--XyZ
Content-Disposition: form-data; name="a"

1
--XyZ
Content-Disposition: form-data; name="f"; filename="a.txt"
Content-Type: text/plain

x
--XyZ--
--FILE--
<?php
foreach ([1, 2] as $i) {
    [$p, $f] = request_parse_body();
    var_dump($p, array_keys($f));
    var_dump(file_get_contents('php://input'));
}
var_dump($_POST, $_FILES);
?>
--EXPECT--
array(1) {
  ["a"]=>
  string(1) "1"
}
array(1) {
  [0]=>
  string(1) "f"
}
string(0) ""
array(0) {
}
array(0) {
}
string(0) ""
array(0) {
}
array(0) {
}
