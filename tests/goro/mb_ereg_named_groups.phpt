--TEST--
mb_ereg(): group name validation and named-group order in $regs
--EXTENSIONS--
mbstring
--FILE--
<?php
set_error_handler(function($n,$s){ echo "  ERR: $s\n"; return true; });
function t($pat, $str, $opt = null, $i = false) {
    if ($opt !== null) mb_regex_set_options($opt);
    $m = null;
    try {
        $r = $i ? mb_eregi($pat, $str, $m) : mb_ereg($pat, $str, $m);
    } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; if ($opt !== null) mb_regex_set_options('pr'); return; }
    echo json_encode([$pat, $str, $opt, $i], JSON_UNESCAPED_UNICODE|JSON_INVALID_UTF8_SUBSTITUTE), " => ", var_export($r, true), " ", json_encode($m, JSON_UNESCAPED_UNICODE|JSON_INVALID_UTF8_SUBSTITUTE), "\n";
    if ($opt !== null) mb_regex_set_options('pr');
}
function ti($pat, $str) { t($pat, $str, null, true); }
t('(?<a$b>x)', 'x');
t('(?<a-b>x)', 'x');
t('(?<1a>x)', 'x');
t('(?<a>x)\k<a$>', 'xx');
t('(?<a>x)\k<1x>', 'xx');
t('(?<a>x)\g<a$>', 'xx');
t('(?<é>x)', 'x');
t('(?<a b>x)', 'x');
t('(?<>x)', 'x');
t('(?<a', 'x');
t('(?<a)', 'x');
t('(x)\k<-1>', 'xx');
t('(x)\k<+1>', 'xx');
t('(x)\k<1+0>', 'xx');
t('(x)\k<-0>', 'xx');
t('(?<n>x)\k<n+1>', 'xx');
t('(?<n>x)\k<n+>', 'xx');
t('(?<n>x)\k<n-a>', 'xx');
t('(?<b>.)(?<a>.)(?<c>.)', 'xyz');
t('(?<zz>.)(?<aa>.)(?<mm>.)(?<bb>.)(?<cc>.)(?<dd>.)(?<ee>.)', 'abcdefg');
t('(?<n>a)|(?<n>b)', 'b');
t('(?<n>a)(?<n>b)', 'ab');
t('(?<n>a)(?<m>b)(?<n>c)', 'abc');
t('(?<x>a)(b)(?<y>c)', 'abc');
$names = [];
for ($i = 0; $i < 70; $i++) $names[] = "(?<g$i>.)";
t(implode('', $names), str_repeat('a', 70));
--EXPECT--
["(?<a$b>x)","x",null,false] => true {"0":"x","1":"x","a$b":"x"}
["(?<a-b>x)","x",null,false] => true {"0":"x","1":"x","a-b":"x"}
  ERR: mb_ereg(): mbregex compile err: invalid group name <1a>
["(?<1a>x)","x",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid char in group name <a$>
["(?<a>x)\\k<a$>","xx",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid group name <1x>
["(?<a>x)\\k<1x>","xx",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: undefined name <a$> reference
["(?<a>x)\\g<a$>","xx",null,false] => false []
["(?<é>x)","x",null,false] => true {"0":"x","1":"x","é":"x"}
["(?<a b>x)","x",null,false] => true {"0":"x","1":"x","a b":"x"}
  ERR: mb_ereg(): mbregex compile err: group name is empty
["(?<>x)","x",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid group name <a>
["(?<a","x",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid group name <a>
["(?<a)","x",null,false] => false []
["(x)\\k<-1>","xx",null,false] => true ["xx","x"]
  ERR: mb_ereg(): mbregex compile err: invalid backref number/name
["(x)\\k<+1>","xx",null,false] => false []
["(x)\\k<1+0>","xx",null,false] => true ["xx","x"]
  ERR: mb_ereg(): mbregex compile err: invalid group name <-0>
["(x)\\k<-0>","xx",null,false] => false []
["(?<n>x)\\k<n+1>","xx",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid group name <n+>>
["(?<n>x)\\k<n+>","xx",null,false] => false []
  ERR: mb_ereg(): mbregex compile err: invalid group name <n-a>>
["(?<n>x)\\k<n-a>","xx",null,false] => false []
["(?<b>.)(?<a>.)(?<c>.)","xyz",null,false] => true {"0":"xyz","1":"x","2":"y","3":"z","a":"y","b":"x","c":"z"}
["(?<zz>.)(?<aa>.)(?<mm>.)(?<bb>.)(?<cc>.)(?<dd>.)(?<ee>.)","abcdefg",null,false] => true {"0":"abcdefg","1":"a","2":"b","3":"c","4":"d","5":"e","6":"f","7":"g","bb":"d","dd":"f","mm":"c","aa":"b","zz":"a","cc":"e","ee":"g"}
["(?<n>a)|(?<n>b)","b",null,false] => true {"0":"b","1":false,"2":"b","n":"b"}
["(?<n>a)(?<n>b)","ab",null,false] => true {"0":"ab","1":"a","2":"b","n":"b"}
["(?<n>a)(?<m>b)(?<n>c)","abc",null,false] => true {"0":"abc","1":"a","2":"b","3":"c","m":"b","n":"c"}
["(?<x>a)(b)(?<y>c)","abc",null,false] => true {"0":"abc","1":"a","2":"c","x":"a","y":"c"}
["(?<g0>.)(?<g1>.)(?<g2>.)(?<g3>.)(?<g4>.)(?<g5>.)(?<g6>.)(?<g7>.)(?<g8>.)(?<g9>.)(?<g10>.)(?<g11>.)(?<g12>.)(?<g13>.)(?<g14>.)(?<g15>.)(?<g16>.)(?<g17>.)(?<g18>.)(?<g19>.)(?<g20>.)(?<g21>.)(?<g22>.)(?<g23>.)(?<g24>.)(?<g25>.)(?<g26>.)(?<g27>.)(?<g28>.)(?<g29>.)(?<g30>.)(?<g31>.)(?<g32>.)(?<g33>.)(?<g34>.)(?<g35>.)(?<g36>.)(?<g37>.)(?<g38>.)(?<g39>.)(?<g40>.)(?<g41>.)(?<g42>.)(?<g43>.)(?<g44>.)(?<g45>.)(?<g46>.)(?<g47>.)(?<g48>.)(?<g49>.)(?<g50>.)(?<g51>.)(?<g52>.)(?<g53>.)(?<g54>.)(?<g55>.)(?<g56>.)(?<g57>.)(?<g58>.)(?<g59>.)(?<g60>.)(?<g61>.)(?<g62>.)(?<g63>.)(?<g64>.)(?<g65>.)(?<g66>.)(?<g67>.)(?<g68>.)(?<g69>.)","aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",null,false] => true {"0":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","1":"a","2":"a","3":"a","4":"a","5":"a","6":"a","7":"a","8":"a","9":"a","10":"a","11":"a","12":"a","13":"a","14":"a","15":"a","16":"a","17":"a","18":"a","19":"a","20":"a","21":"a","22":"a","23":"a","24":"a","25":"a","26":"a","27":"a","28":"a","29":"a","30":"a","31":"a","32":"a","33":"a","34":"a","35":"a","36":"a","37":"a","38":"a","39":"a","40":"a","41":"a","42":"a","43":"a","44":"a","45":"a","46":"a","47":"a","48":"a","49":"a","50":"a","51":"a","52":"a","53":"a","54":"a","55":"a","56":"a","57":"a","58":"a","59":"a","60":"a","61":"a","62":"a","63":"a","64":"a","65":"a","66":"a","67":"a","68":"a","69":"a","70":"a","g64":"a","g56":"a","g48":"a","g65":"a","g57":"a","g49":"a","g66":"a","g58":"a","g67":"a","g59":"a","g68":"a","g10":"a","g69":"a","g11":"a","g0":"a","g1":"a","g20":"a","g12":"a","g2":"a","g21":"a","g13":"a","g30":"a","g22":"a","g14":"a","g3":"a","g31":"a","g23":"a","g15":"a","g4":"a","g24":"a","g16":"a","g5":"a","g25":"a","g17":"a","g6":"a","g40":"a","g32":"a","g26":"a","g18":"a","g7":"a","g41":"a","g33":"a","g50":"a","g19":"a","g8":"a","g42":"a","g34":"a","g51":"a","g9":"a","g43":"a","g35":"a","g27":"a","g52":"a","g44":"a","g36":"a","g60":"a","g28":"a","g45":"a","g37":"a","g61":"a","g29":"a","g53":"a","g46":"a","g38":"a","g62":"a","g54":"a","g47":"a","g39":"a","g63":"a","g55":"a"}
