--TEST--
Argument checks of xml/zlib/hash/openssl/simplexml functions and key_exists()
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => xml_set_element_handler());
t(fn() => xml_set_default_handler(null, null, null));
t(fn() => xml_parse_into_struct(1));
t(fn() => deflate_add(1));
t(fn() => inflate_add());
t(fn() => hash_update(null, null, null));
t(fn() => openssl_cms_verify("a"));
t(fn() => openssl_pkey_get_public(new stdClass));
t(fn() => key_exists([], []));
t(fn() => sscanf("a", "%s", 1));
t(fn() => simplexml_load_string([]));
t(fn() => simplexml_load_string("<a/>", 5));
t(fn() => simplexml_load_string("<a/>", null, "x"));
t(fn() => simplexml_load_file([]));
t(fn() => dom_import_simplexml(1, 2));
t(fn() => Dom\import_simplexml(1, 2));
--EXPECT--
ArgumentCountError: xml_set_element_handler() expects exactly 3 arguments, 0 given
ArgumentCountError: xml_set_default_handler() expects exactly 2 arguments, 3 given
ArgumentCountError: xml_parse_into_struct() expects at least 3 arguments, 1 given
ArgumentCountError: deflate_add() expects at least 2 arguments, 1 given
ArgumentCountError: inflate_add() expects at least 2 arguments, 0 given
ArgumentCountError: hash_update() expects exactly 2 arguments, 3 given
ArgumentCountError: openssl_cms_verify() expects at least 2 arguments, 1 given
Error: Object of class stdClass could not be converted to string
TypeError: key_exists(): Argument #1 ($key) must be a valid array offset type
Error: sscanf(): Argument #3 could not be passed by reference
TypeError: simplexml_load_string(): Argument #1 ($data) must be of type string, array given
TypeError: simplexml_load_string(): Argument #2 ($class_name) must be a class name derived from SimpleXMLElement or null, 5 given
TypeError: simplexml_load_string(): Argument #3 ($options) must be of type int, string given
TypeError: simplexml_load_file(): Argument #1 ($filename) must be of type string, array given
ArgumentCountError: dom_import_simplexml() expects exactly 1 argument, 2 given
ArgumentCountError: Dom\import_simplexml() expects exactly 1 argument, 2 given
