<?php
$out = __DIR__ . '/ref';
@mkdir($out);
$src = '/home/magicaltux/php-src/ext/gd/tests/test.png';
$f = [
    'negate' => [IMG_FILTER_NEGATE],
    'grayscale' => [IMG_FILTER_GRAYSCALE],
    'edge' => [IMG_FILTER_EDGEDETECT],
    'gauss' => [IMG_FILTER_GAUSSIAN_BLUR],
    'selective' => [IMG_FILTER_SELECTIVE_BLUR],
    'emboss' => [IMG_FILTER_EMBOSS],
    'mean' => [IMG_FILTER_MEAN_REMOVAL],
    'smooth' => [IMG_FILTER_SMOOTH, -1924.124],
    'smooth2' => [IMG_FILTER_SMOOTH, 5],
    'colorize' => [IMG_FILTER_COLORIZE, -127, -127, 127],
    'colorize2' => [IMG_FILTER_COLORIZE, 20, -30, 40, 50],
    'contrast' => [IMG_FILTER_CONTRAST, -90],
    'contrast2' => [IMG_FILTER_CONTRAST, 40],
    'brightness' => [IMG_FILTER_BRIGHTNESS, 98],
    'brightness2' => [IMG_FILTER_BRIGHTNESS, -50],
    'pixelate' => [IMG_FILTER_PIXELATE, 5, true],
    'pixelate2' => [IMG_FILTER_PIXELATE, 7, false],
];
foreach ($f as $name => $args) {
    foreach (['tc', 'pal'] as $mode) {
        $im = imagecreatefrompng($src);
        if ($mode == 'pal') {
            imagetruecolortopalette($im, false, 64);
        }
        imagefilter($im, ...$args);
        imagepalettetotruecolor($im);
        imagesavealpha($im, true);
        imagepng($im, "$out/$name-$mode.png");
    }
}
foreach ([[false, 256], [true, 256], [false, 16], [true, 16], [true, 3], [false, 2]] as [$d, $n]) {
    $im = imagecreatefrompng($src);
    imagetruecolortopalette($im, $d, $n);
    echo "topal $d $n colors=", imagecolorstotal($im), "\n";
    imagepalettetotruecolor($im);
    imagesavealpha($im, true);
    imagepng($im, "$out/topal-" . (int)$d . "-$n.png");
}
// transparent color handling
$im = imagecreatefrompng($src);
imagecolortransparent($im, imagecolorat($im, 10, 10));
imagetruecolortopalette($im, true, 50);
echo "trans idx=", imagecolortransparent($im), " total=", imagecolorstotal($im), "\n";
imagepalettetotruecolor($im);
imagesavealpha($im, true);
imagepng($im, "$out/topal-trans.png");
// colormatch
$im = imagecreatefrompng($src);
$im2 = imagecreatefrompng($src);
imagetruecolortopalette($im2, false, 8);
imagecolormatch($im, $im2);
for ($i = 0; $i < imagecolorstotal($im2); $i++) {
    echo implode(',', imagecolorsforindex($im2, $i)), "\n";
}
