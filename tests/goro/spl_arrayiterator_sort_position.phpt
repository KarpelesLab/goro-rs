--TEST--
ArrayIterator sorts keep the iterator's bucket index
--FILE--
<?php
function t($f, $arg = null, $n = 1, $a = [1=>"a",2=>"b",3=>"c",4=>"d"]) {
    $x=new ArrayIterator($a);
    for ($i = 0; $i < $n; $i++) $x->next();
    if ($arg === null) $x->$f(); else $x->$f($arg);
    echo "$f $n: "; var_dump($x->key());
}
foreach ([0,1,2,3] as $n) {
t('uksort', fn($a,$b)=>$b<=>$a, $n);
t('uasort', fn($a,$b)=>$b<=>$a, $n);
t('ksort', null, $n, [4=>"d",2=>"b",3=>"c",1=>"a"]);
t('asort', null, $n, [4=>"d",2=>"b",3=>"c",1=>"a"]);
t('natsort', null, $n, [4=>"d",2=>"b",3=>"c",1=>"a"]);
t('natcasesort', null, $n, [4=>"d",2=>"b",3=>"c",1=>"a"]);
}
--EXPECT--
uksort 0: int(4)
uasort 0: int(4)
ksort 0: int(1)
asort 0: int(1)
natsort 0: int(1)
natcasesort 0: int(1)
uksort 1: int(2)
uasort 1: int(2)
ksort 1: int(2)
asort 1: int(2)
natsort 1: int(2)
natcasesort 1: int(2)
uksort 2: int(1)
uasort 2: int(1)
ksort 2: int(3)
asort 2: int(3)
natsort 2: int(3)
natcasesort 2: int(3)
uksort 3: NULL
uasort 3: NULL
ksort 3: int(4)
asort 3: int(4)
natsort 3: int(4)
natcasesort 3: int(4)
