--TEST--
Redeclaring DOMElement::$tagName untyped fails
--FILE--
<?php
class E extends DOMElement { public $tagName; }
echo "ok\n";
--EXPECTF--

Fatal error: Type of E::$tagName must be string (as in class DOMElement) in %s on line 2
