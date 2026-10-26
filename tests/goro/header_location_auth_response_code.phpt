--TEST--
header(): Location and WWW-Authenticate response codes as sapi_header_op()
--FILE--
<?php

?>
--EXPECT--
302,307,201,401,401,403,301,308
