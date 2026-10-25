--TEST--
iconv: the UTF-16/UTF-32 BOM is written again after a reset
--FILE--
<?php
// glibc writes the BOM of UTF-16/UTF-32 again once the converter was reset
$f = fopen("php://memory", "w+");
stream_filter_append($f, "convert.iconv.UTF-8/UTF-16", STREAM_FILTER_WRITE);
fwrite($f, "ab");
fflush($f);
fwrite($f, "cd");
fflush($f);
fwrite($f, "e");
rewind($f);
var_dump(bin2hex(stream_get_contents($f)));
var_dump(iconv_mime_encode("S", "abcdefghijklmnopqrstuvwxyz", ["output-charset" => "UTF-16", "scheme" => "Q", "line-length" => 40]));
var_dump(iconv_mime_encode("S", "abcdefghijklmnopqrstuvwxyz", ["output-charset" => "UTF-32", "scheme" => "B", "line-length" => 40]));
var_dump(bin2hex(iconv("UTF-8", "UTF-16", "abc")), bin2hex(iconv("UTF-8", "UNICODE", "abc")));
--EXPECT--
string(32) "fffe61006200fffe63006400fffe6500"
string(236) "S: =?UTF-16?Q?=FF=FEa=00b=00c=00d=00?=
 =?UTF-16?Q?=FF=FEe=00f=00g=00h=00i=00?=
 =?UTF-16?Q?=FF=FEj=00k=00l=00m=00n=00?=
 =?UTF-16?Q?=FF=FEo=00p=00q=00r=00s=00?=
 =?UTF-16?Q?=FF=FEt=00u=00v=00w=00x=00?=
 =?UTF-16?Q?=FF=FEy=00z=00?="
string(416) "S: =?UTF-32?B?//4AAGEAAABiAAAA?=
 =?UTF-32?B?//4AAGMAAABkAAAA?=
 =?UTF-32?B?//4AAGUAAABmAAAA?=
 =?UTF-32?B?//4AAGcAAABoAAAA?=
 =?UTF-32?B?//4AAGkAAABqAAAA?=
 =?UTF-32?B?//4AAGsAAABsAAAA?=
 =?UTF-32?B?//4AAG0AAABuAAAA?=
 =?UTF-32?B?//4AAG8AAABwAAAA?=
 =?UTF-32?B?//4AAHEAAAByAAAA?=
 =?UTF-32?B?//4AAHMAAAB0AAAA?=
 =?UTF-32?B?//4AAHUAAAB2AAAA?=
 =?UTF-32?B?//4AAHcAAAB4AAAA?=
 =?UTF-32?B?//4AAHkAAAB6AAAA?="
string(16) "fffe610062006300"
string(16) "fffe610062006300"
