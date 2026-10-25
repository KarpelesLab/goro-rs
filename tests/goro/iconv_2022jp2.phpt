--TEST--
iconv(): ISO-2022-JP-2 character sets, G2 and language tags
--FILE--
<?php
// ISO-2022-JP-2 (glibc): the set in use first, then JIS X 0201 Roman,
// JIS X 0208, JIS X 0212, ISO-8859-1 / -7 upper halves (G2, ESC N),
// GB 2312, KS C 5601; Unicode language tags (ko, zh) reorder them
function e($cs, $s) { $r = @iconv("UTF-8", $cs, $s); return $r === false ? "false" : str_replace("\x1b", "^", $r); }
function d($cs, $s) { $r = @iconv($cs, "UTF-8", $s); return $r === false ? "false" : bin2hex($r); }
function tag($t) { $s = "\u{E0001}"; foreach (str_split($t) as $c) $s .= mb_chr(0xE0000 + ord($c)); return $s; }
foreach (["ISO-2022-JP-2", "CSISO2022JP2", "ISO2022JP2"] as $cs) {
    echo $cs, " ", e($cs, "a日本\u{D55C}日本\u{E9}\u{AB}\u{3B1}\u{386}\u{4E2A}ｱ¥a\\"), "\n";
}
foreach (["", "ja", "ko", "zh", "KO", "ko-KR", "zh-TW", "k", "fr"] as $t) {
    echo str_pad($t, 6), e("ISO-2022-JP-2", tag($t) . "\u{4E00}\u{3B1}\u{E9}\u{A1}\u{A5}\u{386}"), "\n";
}
echo e("ISO-2022-JP-2", tag("ko") . "\u{4E00}\u{E007F}\u{4E00}\n\u{4E00}"), "\n";
echo e("ISO-2022-JP-2", "\u{AB}\u{AB}x\u{AB}\n\u{AB}\r\u{AB}"), "\n";
echo e("ISO-2022-JP-2", "\u{A5}a b\u{203E}\u{A5}\n"), "\n";
echo e("ISO-2022-JP-2", "\u{386}~\u{386}"), "\n";
foreach (["1b2e411b4e41", "1b2e411b4e00", "1b2e41 1b4ec1", "1b2e461b4e41", "1b2e461b4e1f", "1b4e41", "1b2e411b4e410a1b4e41",
          "1b2428442237", "1b242844216e", "1b24284421", "1b2441787e", "1b244178", "1b2849 21", "1b2842 1b2428", "1b242843 3021"] as $h) {
    $h = str_replace(" ", "", $h);
    echo $h, " ", d("ISO-2022-JP-2", hex2bin($h)), "\n";
}
--EXPECT--
ISO-2022-JP-2 a^$BF|K\^$(CGQlm\b^$(D+1^.A^N+^$B&A^$(D&a^$BP$^(I1^N%^(Ba\
CSISO2022JP2 a^$BF|K\^$(CGQlm\b^$(D+1^.A^N+^$B&A^$(D&a^$BP$^(I1^N%^(Ba\
ISO2022JP2 a^$BF|K\^$(CGQlm\b^$(D+1^.A^N+^$B&A^$(D&a^$BP$^(I1^N%^(Ba\
      ^$B0l&A^$(D+1"B^(J\^$(D&a^(B
ja    ^$B0l&A^$(D+1"B^(J\^$(D&a^(B
ko    ^$(Cli%a^.A^Ni".^N%^.F^N6^(B
zh    ^$AR;&A(&^.A^N!^N%^.F^N6^(B
KO    ^$(Cli%a^.A^Ni".^N%^.F^N6^(B
ko-KR ^$(Cli%a^.A^Ni".^N%^.F^N6^(B
zh-TW ^$AR;&A(&^.A^N!^N%^.F^N6^(B
k     ^$B0l&A^$(D+1"B^(J\^$(D&a^(B
fr    ^$B0l&A^$(D+1"B^(J\^$(D&a^(B
^$(Clili^(B
^$B0l^(B
^.A^N+^N+x^N+
^.A^N+^N+
^(J\a^(B b^(J~\^(B

^$(D&a"7&a^(B
1b2e411b4e41 c381
1b2e411b4e00 c280
1b2e411b4ec1 c381
1b2e461b4e41 ce91
1b2e461b4e1f false
1b4e41 false
1b2e411b4e410a1b4e41 c3810ac381
1b2428442237 efbd9e
1b242844216e false
1b24284421 false
1b2441787e false
1b244178 false
1b284921 efbda1
1b28421b2428 false
1b2428433021 eab080
