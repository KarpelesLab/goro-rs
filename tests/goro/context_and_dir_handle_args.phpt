--TEST--
stream_context_*() and readdir()/rewinddir()/closedir() argument errors; stream_is_local() of paths and resources
--FILE--
<?php
$closed = fopen("php://memory", "r"); fclose($closed);
$ctx = stream_context_create();
$f = fopen("php://memory", "r");
$d = opendir(".");
foreach (['stream_context_get_options', 'stream_context_get_params'] as $fn) {
    foreach (['null' => null, 'int' => 1, 'obj' => new stdClass, 'closed' => $closed, 'ctx' => $ctx, 'stream' => $f, 'dir' => $d] as $k => $v) {
        try { echo "$fn($k): ", json_encode($fn($v)), "\n"; } catch (TypeError $e) { echo "$fn($k): ", $e->getMessage(), "\n"; }
    }
}
try { stream_context_set_option(1, "a", "b", "c"); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { stream_context_set_option($ctx, 1, null, 1); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
foreach (['readdir', 'rewinddir', 'closedir'] as $fn) {
    foreach (['int' => 1, 'obj' => new stdClass, 'ctx' => $ctx, 'closed' => $closed, 'stream' => $f] as $k => $v) {
        try { $fn($v); } catch (TypeError $e) { echo "$fn($k): ", $e->getMessage(), "\n"; }
    }
}
var_dump(fclose($d));
closedir($d);
var_dump(stream_is_local("http://x"), stream_is_local("ftp://x"), stream_is_local("data:,x"), stream_is_local("php://memory"), stream_is_local("x.txt"), stream_is_local($f));
var_dump(stream_is_local("foo://x"));
try { stream_is_local(new stdClass); } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump(stream_is_local([]));
try { stream_is_local($closed); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
?>
--EXPECTF--
stream_context_get_options(null): stream_context_get_options(null): stream_context_get_options(): Argument #1 ($stream_or_context) must be of type resource, null given
stream_context_get_options(int): stream_context_get_options(int): stream_context_get_options(): Argument #1 ($stream_or_context) must be of type resource, int given
stream_context_get_options(obj): stream_context_get_options(obj): stream_context_get_options(): Argument #1 ($stream_or_context) must be of type resource, stdClass given
stream_context_get_options(closed): stream_context_get_options(closed): stream_context_get_options(): Argument #1 ($stream_or_context) must be a valid stream/context
stream_context_get_options(ctx): []
stream_context_get_options(stream): []
stream_context_get_options(dir): []
stream_context_get_params(null): stream_context_get_params(null): stream_context_get_params(): Argument #1 ($context) must be of type resource, null given
stream_context_get_params(int): stream_context_get_params(int): stream_context_get_params(): Argument #1 ($context) must be of type resource, int given
stream_context_get_params(obj): stream_context_get_params(obj): stream_context_get_params(): Argument #1 ($context) must be of type resource, stdClass given
stream_context_get_params(closed): stream_context_get_params(closed): stream_context_get_params(): Argument #1 ($context) must be a valid stream/context
stream_context_get_params(ctx): {"options":[]}
stream_context_get_params(stream): {"options":[]}
stream_context_get_params(dir): {"options":[]}
stream_context_set_option(): Argument #1 ($context) must be of type resource, int given
stream_context_set_option(): Argument #3 ($option_name) cannot be null when argument #2 ($wrapper_or_options) is a string
readdir(int): readdir(): Argument #1 ($dir_handle) must be of type resource or null, int given
readdir(obj): readdir(): Argument #1 ($dir_handle) must be of type resource or null, stdClass given
readdir(ctx): readdir(): Argument #1 ($dir_handle) must be an open stream resource
readdir(closed): readdir(): Argument #1 ($dir_handle) must be an open stream resource
readdir(stream): readdir(): Argument #1 ($dir_handle) must be a valid Directory resource
rewinddir(int): rewinddir(): Argument #1 ($dir_handle) must be of type resource or null, int given
rewinddir(obj): rewinddir(): Argument #1 ($dir_handle) must be of type resource or null, stdClass given
rewinddir(ctx): rewinddir(): Argument #1 ($dir_handle) must be an open stream resource
rewinddir(closed): rewinddir(): Argument #1 ($dir_handle) must be an open stream resource
rewinddir(stream): rewinddir(): Argument #1 ($dir_handle) must be a valid Directory resource
closedir(int): closedir(): Argument #1 ($dir_handle) must be of type resource or null, int given
closedir(obj): closedir(): Argument #1 ($dir_handle) must be of type resource or null, stdClass given
closedir(ctx): closedir(): Argument #1 ($dir_handle) must be an open stream resource
closedir(closed): closedir(): Argument #1 ($dir_handle) must be an open stream resource
closedir(stream): closedir(): Argument #1 ($dir_handle) must be a valid Directory resource

Warning: fclose(): cannot close the provided stream, as it must not be manually closed in %s on line %d
bool(false)
bool(false)
bool(false)
bool(false)
bool(true)
bool(true)
bool(true)

Warning: stream_is_local(): Unable to find the wrapper "foo" - did you forget to enable it when you configured PHP? in %s on line %d
bool(true)
Object of class stdClass could not be converted to string

Warning: Array to string conversion in %s on line %d
bool(true)
stream_is_local(): supplied resource is not a valid stream resource
