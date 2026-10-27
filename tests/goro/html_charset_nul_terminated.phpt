--TEST--
htmlspecialchars() & co read their charset as a C string (up to a NUL byte)
--FILE--
<?php
var_dump(htmlspecialchars("<é>", ENT_QUOTES, "\0"));
var_dump(htmlspecialchars("<é>", ENT_QUOTES, "ISO-8859-1\0junk"));
var_dump(htmlentities("<é>", ENT_QUOTES, "UTF-8\0x"));
var_dump(html_entity_decode("&eacute;", ENT_QUOTES, "x\0UTF-8"));
--EXPECTF--
string(10) "&lt;é&gt;"
string(10) "&lt;é&gt;"
string(16) "&lt;&eacute;&gt;"

Warning: html_entity_decode(): Charset "x" is not supported, assuming UTF-8 in %s on line 5
string(2) "é"
