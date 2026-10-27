--TEST--
A cycle through a reference held by storage shared copy-on-write is collected once the other copy is gone
--FILE--
<?php
$z = ['a' => 1];
$z['me'] = &$z;
$w = $z;
unset($z);
echo "collected: ", gc_collect_cycles(), "\n";
unset($w);
echo "collected: ", gc_collect_cycles(), "\n";
$z = ['a' => 1];
$z['me'] = &$z;
unset($z);
echo "collected: ", gc_collect_cycles(), "\n";
--EXPECT--
collected: 0
collected: 1
collected: 1
