--TEST--
class_implements()/class_parents()/class_uses(): the class is reported missing when the autoloader throws
--FILE--
<?php
spl_autoload_register(function ($c) { throw new RuntimeException("fail $c"); });
foreach (["class_implements", "class_parents", "class_uses"] as $f) { try { var_dump($f("Q")); } catch (Exception $e) { echo $e->getMessage(), "\n"; } }
--EXPECTF--

Warning: class_implements(): Class Q does not exist and could not be loaded in %s on line 3
fail Q

Warning: class_parents(): Class Q does not exist and could not be loaded in %s on line 3
fail Q

Warning: class_uses(): Class Q does not exist and could not be loaded in %s on line 3
fail Q
