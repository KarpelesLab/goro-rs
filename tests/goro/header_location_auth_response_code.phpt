--TEST--
header(): Location and WWW-Authenticate response codes as sapi_header_op()
--FILE--
<?php
ob_start(); $r = []; http_response_code(418); header("Location: /x"); $r[] = http_response_code(); header("Location: /y", true, 307); $r[] = http_response_code(); http_response_code(201); header("Location: /z"); $r[] = http_response_code(); http_response_code(200); header("WWW-Authenticate: Basic"); $r[] = http_response_code(); http_response_code(404); header("WWW-Authenticate: Basic"); $r[] = http_response_code(); header("WWW-Authenticate: Basic", true, 403); $r[] = http_response_code(); http_response_code(301); header("Location: /w"); $r[] = http_response_code(); http_response_code(500); header("Location: /v", false, 308); $r[] = http_response_code(); ob_end_clean(); echo implode(",", $r), "\n";
?>
--EXPECT--
302,307,201,401,401,403,301,308
