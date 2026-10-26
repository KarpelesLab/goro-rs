--TEST--
readdir()/rewinddir()/closedir() with no current directory handle: TypeError No resource supplied
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(function () { $d = opendir("."); closedir($d); return readdir(); });
t(fn() => rewinddir());
t(fn() => closedir());
--EXPECTF--

Deprecated: readdir(): Passing null is deprecated, instead the last opened directory stream should be provided in %s on line 3
TypeError: No resource supplied

Deprecated: rewinddir(): Passing null is deprecated, instead the last opened directory stream should be provided in %s on line 4
TypeError: No resource supplied

Deprecated: closedir(): Passing null is deprecated, instead the last opened directory stream should be provided in %s on line 5
TypeError: No resource supplied
