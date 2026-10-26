--TEST--
ReflectionMethod::getFileName() of a method imported from a (nested) trait is the trait's file
--FILE--
<?php
$d = sys_get_temp_dir() . '/refl_trait_file_' . getmypid();
@mkdir($d);
file_put_contents("$d/inner.php", "<?php\ntrait Inner {\n    public function m() {}\n}\n");
file_put_contents("$d/outer.php", "<?php\ntrait Outer {\n    use Inner;\n    public function own() {}\n}\n");
file_put_contents("$d/base.php", "<?php\nclass Base {\n    public function b() {}\n}\n");
require "$d/inner.php";
require "$d/outer.php";
require "$d/base.php";
class C extends Base { use Outer; public function c() {} }
foreach ([['C', 'm'], ['Outer', 'm'], ['Inner', 'm'], ['C', 'own'], ['C', 'b'], ['C', 'c']] as [$cl, $m]) {
    echo "$cl::$m ", basename((new ReflectionMethod($cl, $m))->getFileName()), "\n";
}
unlink("$d/inner.php"); unlink("$d/outer.php"); unlink("$d/base.php"); rmdir($d);

?>
--EXPECTF--
C::m inner.php
Outer::m inner.php
Inner::m inner.php
C::own outer.php
C::b base.php
C::c %s.php
