--TEST--
get_headers(): context type check, empty path, non-URL wrappers
--FILE--
<?php
foreach (["", "abc", "/etc/passwd", "file:///etc/passwd", "php://memory", "foo://bar", "glob://*", "compress.zlib:///x"] as $u) {
  try { var_dump(get_headers($u)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
try { var_dump(get_headers("http://x", false, 5)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(get_headers("abc", false, "x")); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(get_headers("abc", false, null)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
echo "end\n";
--EXPECTF--
ValueError: Path must not be empty

Warning: get_headers(): This function may only be used against URLs in %s on line 3
bool(false)

Warning: get_headers(): This function may only be used against URLs in %s on line 3
bool(false)

Warning: get_headers(): This function may only be used against URLs in %s on line 3
bool(false)

Warning: get_headers(): This function may only be used against URLs in %s on line 3
bool(false)

Warning: get_headers(): Unable to find the wrapper "foo" - did you forget to enable it when you configured PHP? in %s on line 3

Warning: get_headers(): This function may only be used against URLs in %s on line 3
bool(false)

Warning: get_headers(): This function may only be used against URLs in %s on line 3
bool(false)

Warning: get_headers(): This function may only be used against URLs in %s on line 3
bool(false)
TypeError: get_headers(): Argument #3 ($context) must be of type resource or null, int given
TypeError: get_headers(): Argument #3 ($context) must be of type resource or null, string given

Warning: get_headers(): This function may only be used against URLs in %s on line 7
bool(false)
end
