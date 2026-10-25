--TEST--
multipart/form-data: ext/filter registers a field named like an earlier upload
--POST_RAW--
Content-Type: multipart/form-data; boundary=XyZ
--XyZ
Content-Disposition: form-data; name="f"; filename="a.txt"
Content-Type: text/plain

1
--XyZ
Content-Disposition: form-data; name="f"

2
--XyZ
Content-Disposition: form-data; name="g"; filename="b.txt"
Content-Type: text/plain

3
--XyZ
Content-Disposition: form-data; name="g[tmp_name]"

4
--XyZ--
--FILE--
<?php
var_dump($_POST);
var_dump(array_keys($_FILES), $_FILES['f']['size'], $_FILES['g']['name']);
?>
--EXPECT--
array(2) {
  ["f"]=>
  string(1) "2"
  ["g"]=>
  array(1) {
    ["tmp_name"]=>
    string(1) "4"
  }
}
array(2) {
  [0]=>
  string(1) "f"
  [1]=>
  string(1) "g"
}
int(1)
string(5) "b.txt"
