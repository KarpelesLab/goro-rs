--TEST--
ctype_*() deprecation names an anonymous class up to its NUL (zend_zval_type_name)
--FILE--
<?php
$o = new class {};
var_dump(ctype_digit($o), ctype_alpha(new class extends ArrayObject {}));
--EXPECTF--

Deprecated: ctype_digit(): Argument of type class@anonymous will be interpreted as string in the future in %s on line 3

Deprecated: ctype_alpha(): Argument of type ArrayObject@anonymous will be interpreted as string in the future in %s on line 3
bool(false)
bool(false)
