--TEST--
iconv(): ISO-2022-CN and ISO-2022-CN-EXT
--FILE--
<?php
// ISO-2022-CN / ISO-2022-CN-EXT (glibc): GB 2312, CNS 11643 plane 1 (SO),
// plane 2 (SS2) and, in -EXT, ISO-IR-165 (SO) and planes 3-7 (SS3); glibc
// keeps a single shift for the next character of the same plane, shifts
// out from ASCII only, prefers plane 1 once SO designates it, forgets the
// designations at a new line and ends with SI after any designation
function e($cs, $s) { $r = @iconv("UTF-8", $cs, $s); return $r === false ? "false" : strtr($r, ["\x1b" => "^", "\x0e" => "<SO>", "\x0f" => "<SI>", "\n" => "<NL>"]); }
function d($cs, $s) { $r = @iconv($cs, "UTF-8", $s); return $r === false ? "false" : bin2hex($r); }
function di($cs, $s) { $r = @iconv($cs, "UTF-8//IGNORE", $s); return $r === false ? "false" : bin2hex($r); }
foreach (["ISO-2022-CN", "ISO-2022-CN-EXT"] as $cs) {
    echo "== $cs\n";
    foreach (["中文", "中a文", "中\n文", "中a", "乂乂", "乂a乂", "中乂中", "·中", "中·中", "¢a中", "¢中", "㐀㐀a㐀", "中㐀乂", "\u{4E0F}", "\u{3400}\u{4E42}", "中\x1b", "中\u{10FFFF}中", "\u{4E42}\u{309B}"] as $s) {
        echo bin2hex($s), " ", e($cs, $s), " ", e("$cs//TRANSLIT", $s), "\n";
    }
    foreach (["1b242a481b4e212121210f", "0e56500f", "1b2429470a0e56500f", "1b2429410e56500a56500f", "1b2429410e1b24294756500f", "1b242b491b4f43374337",
              "1b4e2180", "1b4e7e7e4141", "0e", "0e41", "1b2429410e21211b2428444142", "1b2429410e7e", "1b2429410e78", "7f", "1b2400", "1b242958"] as $h) {
        echo $h, " ", d($cs, hex2bin($h)), " ", di($cs, hex2bin($h)), "\n";
    }
}
var_dump(iconv_strlen("\x1b\$*H\x1bN!!!!\x0f", "ISO-2022-CN"));
--EXPECT--
== ISO-2022-CN
e4b8ade69687 ^$)A<SO>VPND<SI> ^$)A<SO>VPND<SI>
e4b8ad61e69687 ^$)A<SO>VP<SI>a<SO>ND<SI> ^$)A<SO>VP<SI>a<SO>ND<SI>
e4b8ad0ae69687 ^$)A<SO>VP<SI><NL>^$)A<SO>ND<SI> ^$)A<SO>VP<SI><NL>^$)A<SO>ND<SI>
e4b8ad61 ^$)A<SO>VP<SI>a<SI> ^$)A<SO>VP<SI>a<SI>
e4b982e4b982 ^$*H^N!!!!<SI> ^$*H^N!!!!<SI>
e4b98261e4b982 ^$*H^N!!<SI>a^N!!<SI> ^$*H^N!!<SI>a^N!!<SI>
e4b8ade4b982e4b8ad ^$)A<SO>VP^$*H^N!!VP<SI> ^$)A<SO>VP^$*H^N!!VP<SI>
c2b7e4b8ad ^$)G<SO>!1Dc<SI> ^$)G<SO>!1Dc<SI>
e4b8adc2b7e4b8ad ^$)A<SO>VP^$)G!1Dc<SI> ^$)A<SO>VP^$)G!1Dc<SI>
c2a261e4b8ad false ca^$)A<SO>VP<SI>
c2a2e4b8ad false c^$)A<SO>VP<SI>
e39080e3908061e39080 false ??a?
e4b8ade39080e4b982 false ^$)A<SO>VP<SI>?^$*H^N!!<SI>
e4b88f ^$*H^N!-<SI> ^$*H^N!-<SI>
e39080e4b982 false ?^$*H^N!!<SI>
e4b8ad1b ^$)A<SO>VP<SI>^<SI> ^$)A<SO>VP<SI>^<SI>
e4b8adf48fbfbfe4b8ad false ^$)A<SO>VP<SI>?<SO>VP<SI>
e4b982e3829b false ^$*H^N!!?<SI>
1b242a481b4e212121210f e4b9822121 e4b9822121
0e56500f e4b8ad e4b8ad
1b2429470a0e56500f 0ae78398 0ae78398
1b2429410e56500a56500f false e4b8ade4b8ad
1b2429410e1b24294756500f e4b8ad e4b8ad
1b242b491b4f43374337 1b242b491b4f43374337 1b242b491b4f43374337
1b4e2180 false 21
1b4e7e7e4141 false 7e7e4141
0e  
0e41 false false
1b2429410e21211b2428444142 false false
1b2429410e7e false 
1b2429410e78 false 
7f false 
1b2400 1b2400 1b2400
1b242958 1b242958 1b242958
== ISO-2022-CN-EXT
e4b8ade69687 ^$)A<SO>VPND<SI> ^$)A<SO>VPND<SI>
e4b8ad61e69687 ^$)A<SO>VP<SI>a<SO>ND<SI> ^$)A<SO>VP<SI>a<SO>ND<SI>
e4b8ad0ae69687 ^$)A<SO>VP<SI><NL>^$)A<SO>ND<SI> ^$)A<SO>VP<SI><NL>^$)A<SO>ND<SI>
e4b8ad61 ^$)A<SO>VP<SI>a<SI> ^$)A<SO>VP<SI>a<SI>
e4b982e4b982 ^$*H^N!!!!<SI> ^$*H^N!!!!<SI>
e4b98261e4b982 ^$*H^N!!<SI>a^N!!<SI> ^$*H^N!!<SI>a^N!!<SI>
e4b8ade4b982e4b8ad ^$)A<SO>VP^$*H^N!!VP<SI> ^$)A<SO>VP^$*H^N!!VP<SI>
c2b7e4b8ad ^$)G<SO>!1Dc<SI> ^$)G<SO>!1Dc<SI>
e4b8adc2b7e4b8ad ^$)A<SO>VP^$)G!1Dc<SI> ^$)A<SO>VP^$)G!1Dc<SI>
c2a261e4b8ad ^$)E<SO>!i<SI>a^$)G<SO>Dc<SI> ^$)E<SO>!i<SI>a^$)G<SO>Dc<SI>
c2a2e4b8ad ^$)E<SO>!iVP<SI> ^$)E<SO>!iVP<SI>
e39080e3908061e39080 ^$+L^O",",<SI>a^O",<SI> ^$+L^O",",<SI>a^O",<SI>
e4b8ade39080e4b982 ^$)A<SO>VP^$+L^O",^$*H^N!!<SI> ^$)A<SO>VP^$+L^O",^$*H^N!!<SI>
e4b88f ^$*H^N!-<SI> ^$*H^N!-<SI>
e39080e4b982 ^$+L^O",^$*H^N!!<SI> ^$+L^O",^$*H^N!!<SI>
e4b8ad1b ^$)A<SO>VP<SI>^<SI> ^$)A<SO>VP<SI>^<SI>
e4b8adf48fbfbfe4b8ad false ^$)A<SO>VP<SI>?<SO>VP<SI>
e4b982e3829b false ^$*H^N!!?<SI>
1b242a481b4e212121210f e4b9822121 e4b9822121
0e56500f false 50
1b2429470a0e56500f 0ae78398 0ae78398
1b2429410e56500a56500f false e4b8ad
1b2429410e1b24294756500f e4b8ad e4b8ad
1b242b491b4f43374337 e7a2814337 e7a2814337
1b4e2180 false 
1b4e7e7e4141 false 4141
0e false false
0e41 false 
1b2429410e21211b2428444142 false e38080e8b085
1b2429410e7e false false
1b2429410e78 false false
7f 7f 7f
1b2400 1b2400 1b2400
1b242958 1b242958 1b242958
int(3)
