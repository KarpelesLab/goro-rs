--TEST--
Redeclaring XMLReader::$name untyped fails
--FILE--
<?php
class R extends XMLReader { public $name; }
echo "ok\n";
--EXPECTF--

Fatal error: Type of R::$name must be string (as in class XMLReader) in %s on line 2
