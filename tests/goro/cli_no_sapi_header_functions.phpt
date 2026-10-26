--TEST--
The CLI SAPI has no getallheaders() / apache_request_headers() / apache_response_headers()
--FILE--
<?php
var_dump(PHP_SAPI);
foreach (['getallheaders', 'apache_request_headers', 'apache_response_headers', 'cli_set_process_title'] as $f) {
    echo $f, ': ', var_export(function_exists($f), true), "\n";
}

?>
--EXPECT--
string(3) "cli"
getallheaders: false
apache_request_headers: false
apache_response_headers: false
cli_set_process_title: true
