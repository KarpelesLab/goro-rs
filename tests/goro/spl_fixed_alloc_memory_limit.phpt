--TEST--
SplFixedArray::setSize() over memory_limit is fatal
--INI--
zend.exception_ignore_args=0
zend.exception_string_param_max_len=15
memory_limit=128M
--FILE--
<?php $f = new SplFixedArray(3); $f->setSize(100000000); echo "no";
?>
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 1600000000 bytes) in %s on line 1