--TEST--
Stream functions (PHP_Z_PARAM_STREAM) reject closed streams and non-stream resources, in parameter order
--FILE--
<?php
$closed = fopen("php://memory", "r"); fclose($closed);
$ctx = stream_context_create();
$m = fopen("php://memory", "w+");
foreach (['fflush', 'fpassthru', 'stream_get_contents', 'stream_set_write_buffer', 'set_file_buffer', 'stream_set_read_buffer', 'stream_isatty', 'feof', 'ftell', 'fclose', 'pclose', 'stream_socket_get_name'] as $fn) {
    foreach (['closed' => $closed, 'ctx' => $ctx, 'int' => 1] as $k => $v) {
        $args = [$v];
        if (in_array($fn, ['stream_set_write_buffer', 'set_file_buffer', 'stream_set_read_buffer', 'stream_socket_get_name'])) $args[] = 0;
        try { var_dump($fn(...$args)); } catch (TypeError $e) { echo "$fn($k): ", $e->getMessage(), "\n"; }
    }
}
try { stream_copy_to_stream($closed, 1); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { stream_copy_to_stream($m, $ctx); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { vfprintf($closed, "x", 1); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { fprintf($ctx, "x"); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump(stream_set_write_buffer($m, 0), set_file_buffer($m, 10), stream_set_read_buffer($m, 0), stream_isatty($m));
$p = popen("true", "r");
var_dump(stream_set_write_buffer($p, 0));
pclose($p);
?>
--EXPECT--
fflush(closed): fflush(): Argument #1 ($stream) must be an open stream resource
fflush(ctx): fflush(): Argument #1 ($stream) must be an open stream resource
fflush(int): fflush(): Argument #1 ($stream) must be of type resource, int given
fpassthru(closed): fpassthru(): Argument #1 ($stream) must be an open stream resource
fpassthru(ctx): fpassthru(): Argument #1 ($stream) must be an open stream resource
fpassthru(int): fpassthru(): Argument #1 ($stream) must be of type resource, int given
stream_get_contents(closed): stream_get_contents(): Argument #1 ($stream) must be an open stream resource
stream_get_contents(ctx): stream_get_contents(): Argument #1 ($stream) must be an open stream resource
stream_get_contents(int): stream_get_contents(): Argument #1 ($stream) must be of type resource, int given
stream_set_write_buffer(closed): stream_set_write_buffer(): Argument #1 ($stream) must be an open stream resource
stream_set_write_buffer(ctx): stream_set_write_buffer(): Argument #1 ($stream) must be an open stream resource
stream_set_write_buffer(int): stream_set_write_buffer(): Argument #1 ($stream) must be of type resource, int given
set_file_buffer(closed): set_file_buffer(): Argument #1 ($stream) must be an open stream resource
set_file_buffer(ctx): set_file_buffer(): Argument #1 ($stream) must be an open stream resource
set_file_buffer(int): set_file_buffer(): Argument #1 ($stream) must be of type resource, int given
stream_set_read_buffer(closed): stream_set_read_buffer(): Argument #1 ($stream) must be an open stream resource
stream_set_read_buffer(ctx): stream_set_read_buffer(): Argument #1 ($stream) must be an open stream resource
stream_set_read_buffer(int): stream_set_read_buffer(): Argument #1 ($stream) must be of type resource, int given
stream_isatty(closed): stream_isatty(): Argument #1 ($stream) must be an open stream resource
stream_isatty(ctx): stream_isatty(): Argument #1 ($stream) must be an open stream resource
stream_isatty(int): stream_isatty(): Argument #1 ($stream) must be of type resource, int given
feof(closed): feof(): Argument #1 ($stream) must be an open stream resource
feof(ctx): feof(): Argument #1 ($stream) must be an open stream resource
feof(int): feof(): Argument #1 ($stream) must be of type resource, int given
ftell(closed): ftell(): Argument #1 ($stream) must be an open stream resource
ftell(ctx): ftell(): Argument #1 ($stream) must be an open stream resource
ftell(int): ftell(): Argument #1 ($stream) must be of type resource, int given
fclose(closed): fclose(): Argument #1 ($stream) must be an open stream resource
fclose(ctx): fclose(): Argument #1 ($stream) must be an open stream resource
fclose(int): fclose(): Argument #1 ($stream) must be of type resource, int given
pclose(closed): pclose(): Argument #1 ($handle) must be an open stream resource
pclose(ctx): pclose(): Argument #1 ($handle) must be an open stream resource
pclose(int): pclose(): Argument #1 ($handle) must be of type resource, int given
stream_socket_get_name(closed): stream_socket_get_name(): Argument #1 ($socket) must be an open stream resource
stream_socket_get_name(ctx): stream_socket_get_name(): Argument #1 ($socket) must be an open stream resource
stream_socket_get_name(int): stream_socket_get_name(): Argument #1 ($socket) must be of type resource, int given
stream_copy_to_stream(): Argument #1 ($from) must be an open stream resource
stream_copy_to_stream(): Argument #2 ($to) must be an open stream resource
vfprintf(): Argument #1 ($stream) must be an open stream resource
fprintf(): Argument #1 ($stream) must be an open stream resource
int(-1)
int(-1)
int(0)
bool(false)
int(0)
