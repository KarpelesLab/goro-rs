--TEST--
A stream wrapper's __call gets stream_open's opened_path as a reference in its arguments array
--FILE--
<?php
class W {
    public $context;
    public function __call(string $name, array $arguments): mixed {
        if ($name === 'stream_open') {
            $h = function (string $path, string $mode, int $options, ?string &$openedPath): bool {
                $openedPath = 'x';
                return true;
            };
            return call_user_func_array($h, $arguments);
        }
        if ($name === 'stream_write') {
            return strlen($arguments[0]);
        }
        if ($name === 'stream_eof') {
            return true;
        }
        return false;
    }
}
stream_wrapper_register('test', 'W');
$f = fopen('test://foo', 'w');
var_dump(is_resource($f));
var_dump(fwrite($f, "hello"));
fclose($f);

?>
--EXPECT--
bool(true)
int(5)
