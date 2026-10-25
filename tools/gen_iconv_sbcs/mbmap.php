<?php
// exhaustive BMP encode + 1/2-byte decode for multibyte charsets
$list = $argv[1] ?? "SJIS,CP932,EUC-JP,EUC-JP-MS,EUC-KR,UHC,BIG5,CP950,GB2312,GBK,ISO-2022-JP,CP1258";
foreach (explode(",", $list) as $cs) {
  for ($cp = 0; $cp < 0x10000; $cp++) {
    if ($cp >= 0xD800 && $cp < 0xE000) continue;
    $r = @iconv("UTF-32BE", $cs, pack('N', $cp));
    if ($r !== false) echo "$cs E ", dechex($cp), " ", bin2hex($r), "\n";
  }
  for ($b = 0; $b < 0x10000; $b++) {
    $s = $b < 0x100 ? chr($b) : pack('n', $b);
    if ($b >= 0x100 && ($b >> 8) < 0x80) continue;
    error_clear_last();
    $r = @iconv($cs, "UTF-32BE", $s);
    if ($r === false) { $e = error_get_last(); $r = str_contains($e['message'] ?? '', 'incomplete') ? "INC" : "ILL"; }
    else $r = bin2hex($r);
    if ($b >= 0x100 && $r === "ILL") continue;
    echo "$cs D ", dechex($b), " $r\n";
  }
}
