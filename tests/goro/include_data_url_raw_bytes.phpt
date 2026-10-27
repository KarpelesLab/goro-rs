--TEST--
include of a data: URL keeps its raw bytes
--INI--
allow_url_include=1
--FILE--
<?php
include "data:text/plain,<?php echo bin2hex('r\xe9\xff'), PHP_EOL;";
include "data:text/plain,<?php echo bin2hex(\"\xe9\"), PHP_EOL;";
?>
--EXPECT--
Deprecated: Directive 'allow_url_include' is deprecated in Unknown on line 0
72e9ff
e9
