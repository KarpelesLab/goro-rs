--TEST--
finfo::file('') reports argument #2 like php ($flags for the method)
--FILE--
<?php
$f = new finfo(FILEINFO_MIME_TYPE);
try { $f->file(''); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
try { finfo_file($f, ''); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
try { mime_content_type(''); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
?>
--EXPECT--
finfo::file(): Argument #2 ($flags) must not be empty
finfo_file(): Argument #2 ($filename) must not be empty
mime_content_type(): Argument #1 ($filename) must not be empty
