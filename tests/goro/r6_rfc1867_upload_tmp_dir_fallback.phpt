--TEST--
multipart/form-data: an unusable upload_tmp_dir falls back to the system temporary directory
--INI--
upload_tmp_dir=/nonexistent/goro/dir
display_errors=1
--POST_RAW--
Content-Type: multipart/form-data; boundary=XyZ
--XyZ
Content-Disposition: form-data; name="f"; filename="a.txt"
Content-Type: text/plain

x
--XyZ--
--FILE--
<?php
var_dump($_FILES['f']['error'], $_FILES['f']['size'], is_uploaded_file($_FILES['f']['tmp_name']));
var_dump(error_get_last()['message']);
?>
--EXPECT--
Notice: PHP Request Startup: file created in the system's temporary directory in Unknown on line 0
int(0)
int(1)
bool(true)
string(69) "PHP Request Startup: file created in the system's temporary directory"
