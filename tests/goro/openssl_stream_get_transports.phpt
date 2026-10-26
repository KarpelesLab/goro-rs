--TEST--
stream_get_transports() lists the openssl transports
--EXTENSIONS--
openssl
--FILE--
<?php
echo implode(",", stream_get_transports()), "\n";
?>
--EXPECT--
tcp,udp,unix,udg,ssl,tls,tlsv1.0,tlsv1.1,tlsv1.2,tlsv1.3
