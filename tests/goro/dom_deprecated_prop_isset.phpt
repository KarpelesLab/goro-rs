--TEST--
isset()/??/empty() on a native property do not silence its diagnostics
--FILE--
<?php $d = new DOMDocument; $d->loadXML("<!DOCTYPE r [<!ENTITY e \"v\">]><r/>"); $e = $d->doctype->entities->item(0); var_dump($e->version); var_dump($e->version ?? 1); var_dump(isset($e->version)); var_dump(empty($e->encoding)); var_dump(isset($d->config), $d->config ?? 2, isset($d->actualEncoding));
--EXPECTF--

Deprecated: Property DOMEntity::$version is deprecated in %s on line 1
NULL

Deprecated: Property DOMEntity::$version is deprecated in %s on line 1
int(1)

Deprecated: Property DOMEntity::$version is deprecated in %s on line 1
bool(false)

Deprecated: Property DOMEntity::$encoding is deprecated in %s on line 1
bool(true)

Deprecated: Property DOMDocument::$config is deprecated in %s on line 1

Deprecated: Property DOMDocument::$config is deprecated in %s on line 1

Deprecated: Property DOMDocument::$actualEncoding is deprecated in %s on line 1
bool(false)
int(2)
bool(false)
