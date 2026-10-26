--TEST--
$context arguments: non-resources are type errors; a resource that is no context throws after the function ran without one
--FILE--
<?php
$d = sys_get_temp_dir() . "/ctxchk_" . getmypid();
@mkdir($d);
chdir($d);
file_put_contents("f", "");
$m = fopen("php://memory", "r");
$calls = [
    'fopen' => fn($c) => fopen("f", "r", false, $c),
    'file_get_contents' => fn($c) => file_get_contents("f", false, $c),
    'file_put_contents' => fn($c) => file_put_contents("f", "x", 0, $c),
    'file' => fn($c) => file("f", 0, $c),
    'readfile' => fn($c) => readfile("f", false, $c),
    'opendir' => fn($c) => opendir(".", $c),
    'scandir' => fn($c) => scandir(".", 0, $c),
    'mkdir' => fn($c) => mkdir("sub", 0777, false, $c),
    'rmdir' => fn($c) => rmdir("sub", $c),
    'unlink' => fn($c) => unlink("nonexist", $c),
    'copy' => fn($c) => copy("f", "g", $c),
];
foreach ($calls as $n => $f) {
    foreach ([$m, 1] as $c) {
        try { $f($c); echo "$n: no error\n"; } catch (TypeError $e) { echo "\n$n: ", $e->getMessage(), "\n"; }
    }
}
var_dump(is_dir("sub"), file_exists("g"));
@unlink("g"); unlink("f"); @rmdir("sub");
chdir("/");
rmdir($d);
?>
--EXPECTF--
fopen: fopen(): supplied resource is not a valid Stream-Context resource

fopen: fopen(): Argument #4 ($context) must be of type resource or null, int given

file_get_contents: file_get_contents(): supplied resource is not a valid Stream-Context resource

file_get_contents: file_get_contents(): Argument #3 ($context) must be of type resource or null, int given

file_put_contents: file_put_contents(): supplied resource is not a valid Stream-Context resource

file_put_contents: file_put_contents(): Argument #4 ($context) must be of type resource or null, int given

file: file(): supplied resource is not a valid Stream-Context resource

file: file(): Argument #3 ($context) must be of type resource or null, int given
x
readfile: readfile(): supplied resource is not a valid Stream-Context resource

readfile: readfile(): Argument #3 ($context) must be of type resource or null, int given

opendir: opendir(): supplied resource is not a valid Stream-Context resource

opendir: opendir(): Argument #2 ($context) must be of type resource or null, int given

scandir: scandir(): supplied resource is not a valid Stream-Context resource

scandir: scandir(): Argument #3 ($context) must be of type resource or null, int given

mkdir: mkdir(): supplied resource is not a valid Stream-Context resource

mkdir: mkdir(): Argument #4 ($context) must be of type resource or null, int given

rmdir: rmdir(): supplied resource is not a valid Stream-Context resource

rmdir: rmdir(): Argument #2 ($context) must be of type resource or null, int given

Warning: unlink(nonexist): No such file or directory in %s on line %d

unlink: unlink(): supplied resource is not a valid Stream-Context resource

unlink: unlink(): Argument #2 ($context) must be of type resource or null, int given

copy: copy(): supplied resource is not a valid Stream-Context resource

copy: copy(): Argument #3 ($context) must be of type resource or null, int given
bool(false)
bool(true)
