--TEST--
ob_gzhandler: the accepted encoding is the request's, not a $_SERVER entry the script sets
--FILE--
<?php
$_SERVER['HTTP_ACCEPT_ENCODING'] = 'gzip';
ob_start('ob_gzhandler');
echo "hello\n";
?>
--EXPECT--
hello
