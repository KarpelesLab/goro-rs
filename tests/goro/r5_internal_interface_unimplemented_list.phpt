--TEST--
Unimplemented internal interface methods are listed in table order
--FILE--
<?php
class B4 implements SeekableIterator {}
--EXPECTF--

Fatal error: Class B4 contains 6 abstract methods and must therefore be declared abstract or implement the remaining methods (SeekableIterator::seek, Iterator::current, Iterator::next, ...) in %s on line 2
