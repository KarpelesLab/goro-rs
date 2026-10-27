--TEST--
imagecopyresampled() within one image reads the pixels it already wrote (as libgd does)
--EXTENSIONS--
gd
--FILE--
<?php
$im = imagecreatetruecolor(4, 1);
foreach ([0x110000, 0x002200, 0x000033, 0x444444] as $x => $c) imagesetpixel($im, $x, 0, $c);
imagecopyresampled($im, $im, 1, 0, 0, 0, 2, 1, 2, 1);
for ($x = 0; $x < 4; $x++) printf("%06x\n", imagecolorat($im, $x, 0));
?>
--EXPECT--
110000
110000
110000
444444
