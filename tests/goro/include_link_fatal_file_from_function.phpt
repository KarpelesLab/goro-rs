--TEST--
Link-time fatal error in a file included by a function of another file names that file
--FILE--
<?php
$d = sys_get_temp_dir() . '/goro_inc_fatal_' . getmypid();
@mkdir($d);
file_put_contents("$d/A.php", "<?php\nnamespace N;\nclass A { public function f(): void {} }\n");
file_put_contents("$d/B.php", "<?php\nnamespace N;\n\nclass B extends A\n{\n    protected function f(): void {}\n}\n");
file_put_contents("$d/T.php", "<?php\nfunction t() {\n    require_once 'A.php';\n    require_once 'B.php';\n}\n");
register_shutdown_function(function () use ($d) {
    foreach (['A', 'B', 'T'] as $f) unlink("$d/$f.php");
    rmdir($d);
    echo "\nshutdown\n";
});
require "$d/T.php";
t();

?>
--EXPECTF--
Fatal error: Access level to N\B::f() must be public (as in class N\A) in %sgoro_inc_fatal_%d%eB.php on line 6
%A
shutdown
