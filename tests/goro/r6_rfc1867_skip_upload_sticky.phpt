--TEST--
multipart/form-data: after a file part with a malformed name, later uploads are skipped too
--POST_RAW--
Content-Type: multipart/form-data; boundary=XyZ
--XyZ
Content-Disposition: form-data; name="g"; filename="b.txt"
Content-Type: text/plain

y
--XyZ
Content-Disposition: form-data; name="f[a"; filename="a.txt"
Content-Type: text/plain

x
--XyZ
Content-Disposition: form-data; name="h"; filename="c.txt"
Content-Type: text/plain

z
--XyZ
Content-Disposition: form-data; name="field"

still parsed
--XyZ--
--FILE--
<?php
var_dump(array_keys($_FILES), $_POST);
?>
--EXPECT--
array(1) {
  [0]=>
  string(1) "g"
}
array(1) {
  ["field"]=>
  string(12) "still parsed"
}
