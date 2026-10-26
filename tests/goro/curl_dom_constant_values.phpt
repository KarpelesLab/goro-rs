--TEST--
Constant values: the libcurl ~masks are sign-extended longs; XML_ATTRIBUTE_ENTITY is XML_ATTRIBUTE_ENTITIES
--EXTENSIONS--
curl
dom
--FILE--
<?php
var_dump(CURLAUTH_ANY, CURLAUTH_ANYSAFE, CURLPROTO_ALL, CURLSSH_AUTH_ANY, CURLSSH_AUTH_DEFAULT, XML_ATTRIBUTE_ENTITY);
?>
--EXPECT--
int(-17)
int(-18)
int(-1)
int(-1)
int(-1)
int(6)
