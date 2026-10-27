--TEST--
SplFileInfo class arguments go through try_convert_to_string(); openFile() checks $useIncludePath/$context itself
--FILE--
<?php
function t($c) { try { var_dump($c()); } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; } }
$i = new SplFileInfo('/tmp/nonexistent/x.tar.gz');
foreach ([new stdClass, [], [1], NAN, 1.5, true, false, 5, "a\0b"] as $v) {
  t(fn() => $i->getPathInfo($v));
  t(fn() => $i->setInfoClass($v));
  t(fn() => $i->setFileClass($v));
  t(fn() => $i->getFileInfo($v));
}
t(fn() => $i->openFile([], 5));
t(fn() => $i->openFile("r", new stdClass));
t(fn() => $i->openFile("r", false, 5));
t(fn() => $i->openFile("r", false, null));
t(fn() => $i->openFile("r", "x", "y"));
t(fn() => $i->openFile(NAN));
t(fn() => $i->openFile("r", 1.5));
t(fn() => $i->openFile("r", false, null, 4));
--EXPECTF--
Error: Object of class stdClass could not be converted to string
Error: Object of class stdClass could not be converted to string
Error: Object of class stdClass could not be converted to string
Error: Object of class stdClass could not be converted to string

Warning: Array to string conversion in %s on line 5
TypeError: SplFileInfo::getPathInfo(): Argument #1 ($class) must be a valid class name or null, Array given

Warning: Array to string conversion in %s on line 6
TypeError: SplFileInfo::setInfoClass(): Argument #1 ($class) must be a class name derived from SplFileInfo, Array given

Warning: Array to string conversion in %s on line 7
TypeError: SplFileInfo::setFileClass(): Argument #1 ($class) must be a class name derived from SplFileObject, Array given

Warning: Array to string conversion in %s on line 8
TypeError: SplFileInfo::getFileInfo(): Argument #1 ($class) must be a class name derived from SplFileInfo or null, Array given

Warning: Array to string conversion in %s on line 5
TypeError: SplFileInfo::getPathInfo(): Argument #1 ($class) must be a valid class name or null, Array given

Warning: Array to string conversion in %s on line 6
TypeError: SplFileInfo::setInfoClass(): Argument #1 ($class) must be a class name derived from SplFileInfo, Array given

Warning: Array to string conversion in %s on line 7
TypeError: SplFileInfo::setFileClass(): Argument #1 ($class) must be a class name derived from SplFileObject, Array given

Warning: Array to string conversion in %s on line 8
TypeError: SplFileInfo::getFileInfo(): Argument #1 ($class) must be a class name derived from SplFileInfo or null, Array given

Warning: unexpected NAN value was coerced to string in %s on line 5
TypeError: SplFileInfo::getPathInfo(): Argument #1 ($class) must be a valid class name or null, NAN given

Warning: unexpected NAN value was coerced to string in %s on line 6
TypeError: SplFileInfo::setInfoClass(): Argument #1 ($class) must be a class name derived from SplFileInfo, NAN given

Warning: unexpected NAN value was coerced to string in %s on line 7
TypeError: SplFileInfo::setFileClass(): Argument #1 ($class) must be a class name derived from SplFileObject, NAN given

Warning: unexpected NAN value was coerced to string in %s on line 8
TypeError: SplFileInfo::getFileInfo(): Argument #1 ($class) must be a class name derived from SplFileInfo or null, NAN given
TypeError: SplFileInfo::getPathInfo(): Argument #1 ($class) must be a valid class name or null, 1.5 given
TypeError: SplFileInfo::setInfoClass(): Argument #1 ($class) must be a class name derived from SplFileInfo, 1.5 given
TypeError: SplFileInfo::setFileClass(): Argument #1 ($class) must be a class name derived from SplFileObject, 1.5 given
TypeError: SplFileInfo::getFileInfo(): Argument #1 ($class) must be a class name derived from SplFileInfo or null, 1.5 given
TypeError: SplFileInfo::getPathInfo(): Argument #1 ($class) must be a valid class name or null, 1 given
TypeError: SplFileInfo::setInfoClass(): Argument #1 ($class) must be a class name derived from SplFileInfo, 1 given
TypeError: SplFileInfo::setFileClass(): Argument #1 ($class) must be a class name derived from SplFileObject, 1 given
TypeError: SplFileInfo::getFileInfo(): Argument #1 ($class) must be a class name derived from SplFileInfo or null, 1 given
TypeError: SplFileInfo::getPathInfo(): Argument #1 ($class) must be a valid class name or null,  given
TypeError: SplFileInfo::setInfoClass(): Argument #1 ($class) must be a class name derived from SplFileInfo,  given
TypeError: SplFileInfo::setFileClass(): Argument #1 ($class) must be a class name derived from SplFileObject,  given
TypeError: SplFileInfo::getFileInfo(): Argument #1 ($class) must be a class name derived from SplFileInfo or null,  given
TypeError: SplFileInfo::getPathInfo(): Argument #1 ($class) must be a valid class name or null, 5 given
TypeError: SplFileInfo::setInfoClass(): Argument #1 ($class) must be a class name derived from SplFileInfo, 5 given
TypeError: SplFileInfo::setFileClass(): Argument #1 ($class) must be a class name derived from SplFileObject, 5 given
TypeError: SplFileInfo::getFileInfo(): Argument #1 ($class) must be a class name derived from SplFileInfo or null, 5 given
TypeError: SplFileInfo::getPathInfo(): Argument #1 ($class) must be a valid class name or null, a given
TypeError: SplFileInfo::setInfoClass(): Argument #1 ($class) must be a class name derived from SplFileInfo, a given
TypeError: SplFileInfo::setFileClass(): Argument #1 ($class) must be a class name derived from SplFileObject, a given
TypeError: SplFileInfo::getFileInfo(): Argument #1 ($class) must be a class name derived from SplFileInfo or null, a given
TypeError: SplFileInfo::openFile(): Argument #1 ($mode) must be of type string, array given
TypeError: SplFileInfo::openFile(): Argument #2 ($useIncludePath) must be of type bool, stdClass given
TypeError: SplFileInfo::openFile(): Argument #3 ($context) must be of type resource or null, int given
RuntimeException: SplFileInfo::openFile(/tmp/nonexistent/x.tar.gz): Failed to open stream: No such file or directory
TypeError: SplFileInfo::openFile(): Argument #3 ($context) must be of type resource or null, string given

Warning: unexpected NAN value was coerced to string in %s on line 15
RuntimeException: SplFileInfo::openFile(/tmp/nonexistent/x.tar.gz): Failed to open stream: `NAN' is not a valid mode for fopen
RuntimeException: SplFileInfo::openFile(/tmp/nonexistent/x.tar.gz): Failed to open stream: No such file or directory
ArgumentCountError: SplFileInfo::openFile() expects at most 3 arguments, 4 given
