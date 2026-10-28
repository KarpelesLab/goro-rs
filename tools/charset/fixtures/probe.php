<?php
// Prints how single code points encode: "ENC CP ILLEGAL HEX" per line.
// Usage: php probe.php "ENC:LO-HI[,ENC:LO-HI...]"  (hex code points)
error_reporting(E_ALL & ~E_DEPRECATED);
mb_substitute_character(0x25);
foreach (explode(',', $argv[1]) as $item) {
    $p = strrpos($item, ':');
    $name = substr($item, 0, $p);
    [$lo, $hi] = array_map('hexdec', explode('-', substr($item, $p + 1)));
    for ($cp = $lo; $cp <= $hi; $cp++) {
        $ill = mb_get_info('illegal_chars');
        $out = mb_convert_encoding(pack('N', $cp), $name, 'UTF-32BE');
        printf("%s %04x %d %s\n", $name, $cp, mb_get_info('illegal_chars') - $ill, bin2hex($out));
    }
}
