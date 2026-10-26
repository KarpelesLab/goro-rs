--TEST--
stream_get_wrappers() in registration order; openssl's transports listed
--FILE--
<?php
class W { public $context; function stream_open() { return true; } }
stream_wrapper_register("myw", "W");
$w = array_values(array_diff(stream_get_wrappers(), ['zip', 'compress.bzip2']));
echo implode(",", $w), "\n";
stream_wrapper_unregister("http");
$w = array_values(array_diff(stream_get_wrappers(), ['zip', 'compress.bzip2']));
echo implode(",", $w), "\n";
echo implode(",", stream_get_transports()), "\n";
--EXPECT--
https,ftps,compress.zlib,php,file,glob,data,http,ftp,phar,myw
https,ftps,compress.zlib,php,file,glob,data,ftp,phar,myw
tcp,udp,unix,udg,ssl,tls,tlsv1.0,tlsv1.1,tlsv1.2,tlsv1.3
