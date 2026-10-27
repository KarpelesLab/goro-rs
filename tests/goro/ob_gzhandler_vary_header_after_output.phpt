--TEST--
ob_gzhandler without an accepted encoding adds Vary: Accept-Encoding, refused once output started
--FILE--
<?php
echo "start\n";
ob_start('ob_gzhandler'); echo "gz"; ob_end_flush(); echo "\n";
ob_start('ob_gzhandler'); echo "gz2"; ob_end_clean(); echo "\n";
var_dump(ob_gzhandler("data", PHP_OUTPUT_HANDLER_START));
$_SERVER['HTTP_ACCEPT_ENCODING'] = 'gzip';
ob_start('ob_gzhandler'); echo "gz3"; ob_end_flush(); echo "\n";
--EXPECTF--
start
gz
Warning: Cannot modify header information - headers already sent by (output started at %s:2) in %s on line 3


bool(false)
gz3
Warning: Cannot modify header information - headers already sent by (output started at %s:2) in %s on line 7

