--TEST--
Objects of user classes SPL creates natively (iterator class, getChildren, info/file class) have uninitialized typed slots and run their destructors
--FILE--
<?php
class MyAI extends ArrayIterator { public int $t; public $p = 'p'; function __construct($a = [], $f = 0) { echo "MyAI ctor\n"; parent::__construct($a, $f); } function __get($n) { echo "__get $n\n"; return 1; } function __destruct() { echo "MyAI dtor\n"; } }
$ao = new ArrayObject([1, 2]); $ao->setIteratorClass('MyAI');
$it = $ao->getIterator(); var_dump(get_class($it), $it->p);
try { var_dump($it->t); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
unset($it); echo "after unset\n";
class MyRAI extends RecursiveArrayIterator { public int $t; public $depth = 0; function __destruct() { echo "MyRAI dtor\n"; } }
$r = new MyRAI([1, [2, [3]]]);
foreach (new RecursiveIteratorIterator($r) as $v) echo $v; echo "\n";
$r->rewind(); $r->next(); $c = $r->getChildren(); var_dump(get_class($c), $c->depth);
try { var_dump($c->t); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
unset($c); echo "after children\n";
class MyInfo extends SplFileInfo { public int $t; public $x = 'x'; function __construct($f) { echo "MyInfo ctor\n"; parent::__construct($f); } function __destruct() { echo "MyInfo dtor\n"; } }
$fi = new SplFileInfo(__FILE__); $fi->setInfoClass('MyInfo');
$i2 = $fi->getFileInfo(); var_dump(get_class($i2), $i2->x);
try { var_dump($i2->t); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
unset($i2); echo "after info\n";
$p = $fi->getPathInfo(); var_dump(get_class($p)); unset($p);
class MyFile extends SplFileObject { public int $t; public $y = 'y'; function __destruct() { echo "MyFile dtor\n"; } }
$fi->setFileClass('MyFile'); $f = $fi->openFile(); var_dump(get_class($f), $f->y);
try { var_dump($f->t); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
unset($f); echo "after file\n";
$di = new FilesystemIterator(__DIR__, FilesystemIterator::CURRENT_AS_FILEINFO); $di->setInfoClass('MyInfo');
foreach ($di as $e) { var_dump(get_class($e)); break; }
unset($e, $di); echo "after dir\n";
class MyX extends SimpleXMLElement { public function __destruct() { } }
echo "end\n";
?>
--EXPECT--
string(4) "MyAI"
string(1) "p"
Error: Typed property MyAI::$t must not be accessed before initialization
MyAI dtor
after unset
123MyRAI dtor
MyRAI dtor

string(5) "MyRAI"
int(0)
Error: Typed property MyRAI::$t must not be accessed before initialization
MyRAI dtor
after children
MyInfo ctor
string(6) "MyInfo"
string(1) "x"
Error: Typed property MyInfo::$t must not be accessed before initialization
MyInfo dtor
after info
MyInfo ctor
string(6) "MyInfo"
MyInfo dtor
string(6) "MyFile"
string(1) "y"
Error: Typed property MyFile::$t must not be accessed before initialization
MyFile dtor
after file
MyInfo ctor
string(6) "MyInfo"
MyInfo dtor
after dir
end
MyRAI dtor
