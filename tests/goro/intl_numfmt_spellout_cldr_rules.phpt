--TEST--
NumberFormatter::SPELLOUT with CLDR's rules: every locale, fractions (ru целых, pl numerators), rule set attributes
--EXTENSIONS--
intl
--FILE--
<?php
// whole numbers of intl's locales go through intl; the other locales and
// the fractions run CLDR's SpelloutRules: x.x rules by digits, ru's and
// pl's fraction rule sets (closest denominator), the x,x rule of a comma
// decimal separator, Inf / NaN rules, CLDR's parent locales (nb's are
// no's), root's digits for a locale without rules (ur)
foreach (["en", "ru", "pl", "es", "es_MX", "ar", "hi", "zh", "ja", "ko", "tr", "he", "th", "vi", "nb", "sw", "ur"] as $l) {
    $f = new NumberFormatter($l, NumberFormatter::SPELLOUT);
    $out = [];
    foreach ([21, 1234, -5, 1.5, 3.25, 0.05, 2024.75] as $n) {
        $out[] = $f->format($n);
    }
    echo $l, ": ", implode(" | ", $out), "\n";
}
$f = new NumberFormatter("en", NumberFormatter::SPELLOUT);
echo $f->format(NAN), " | ", $f->format(-INF), " | ", $f->format(PHP_INT_MAX), "\n";
echo $f->getTextAttribute(NumberFormatter::DEFAULT_RULESET), "\n";
echo $f->getTextAttribute(NumberFormatter::PUBLIC_RULESETS), "\n";
foreach (["%spellout-ordinal", "%spellout-numbering-year", "%spellout-cardinal-verbose", "%bogus", "%%2d-year"] as $rs) {
    $f = new NumberFormatter("en", NumberFormatter::SPELLOUT);
    var_dump($f->setTextAttribute(NumberFormatter::DEFAULT_RULESET, $rs));
    echo $rs, ": ", $f->format(1999), " / ", $f->format(2005), " / ", $f->getTextAttribute(NumberFormatter::DEFAULT_RULESET), "\n";
}
$o = new NumberFormatter("fr", NumberFormatter::ORDINAL);
echo $o->getTextAttribute(NumberFormatter::PUBLIC_RULESETS), "\n";
$o->setTextAttribute(NumberFormatter::DEFAULT_RULESET, "%digits-ordinal-feminine");
echo $o->format(1), " ", $o->format(2), "\n";
// custom rules: the default rule set is the last public one
$r = new NumberFormatter("en", NumberFormatter::PATTERN_RULEBASED, "%a: 0: zero; 1: one; %b: 0: nul; 1: een;");
echo $r->format(1), " ", $r->getTextAttribute(NumberFormatter::DEFAULT_RULESET), " ", $r->getTextAttribute(NumberFormatter::PUBLIC_RULESETS), "\n";
?>
--EXPECT--
en: twenty-one | one thousand two hundred thirty-four | minus five | one point five | three point two five | zero point zero five | two thousand twenty-four point seven five
ru: двадцать один | одна тысяча двести тридцать четыре | минус пять | одна целая пять десятых | три целых двадцать пять сотых | пять сотых | две тысячи двадцать четыре целых семьдесят пять сотых
pl: dwadzieścia jeden | tysiąc dwieście trzydzieści cztery | minus pięć | jeden przecinek pięć | trzy przecinek dwadzieścia pięć | zero przecinek zero pięć | dwa tysiące dwadzieścia cztery przecinek siedemdziesiąt pięć
es: veintiuno | mil doscientos treinta y cuatro | menos cinco | uno coma cinco | tres coma dos cinco | cero coma cero cinco | dos mil veinticuatro coma siete cinco
es_MX: veintiuno | mil doscientos treinta y cuatro | menos cinco | uno punto cinco | tres punto dos cinco | cero punto cero cinco | dos mil veinticuatro punto siete cinco
ar: واحد وعشرون | ألف ومائتان وأربعة وثلاثون | ناقص خمسة | واحد فاصل خمسة | ثلاثة فاصل إثنان خمسة | صفر فاصل صفر خمسة | ألفين وأربعة وعشرون فاصل سبعة خمسة
hi: इक्कीस | एक हज़ार दो सौ चौंतीस | ऋण पाँच | एक दशमलव पाँच | तीन दशमलव दो पाँच | शून्य दशमलव शून्य पाँच | दो हज़ार चौबीस दशमलव सात पाँच
zh: 二十一 | 一千二百三十四 | 负五 | 一点五 | 三点二五 | 〇点〇五 | 二千〇二十四点七五
ja: 二十一 | 千二百三十四 | マイナス五 | 一・五 | 三・二五 | 〇・〇五 | 二千二十四・七五
ko: 이십일 | 천이백삼십사 | 마이너스 오 | 일점오 | 삼점이오 | 영점공오 | 이천이십사점칠오
tr: yirmi bir | bin iki yüz otuz dört | eksi beş | bir virgül beş | üç virgül iki beş | sıfır virgül sıfır beş | iki bin yirmi dört virgül yedi beş
he: עשרים ואחת | אלף מאתיים שלושים וארבע | מינוס חמש | אחת נקודה חמש | שלוש נקודה שתיים חמש | אפס נקודה אפס חמש | אלפיים עשרים וארבע נקודה שבע חמש
th: ยี่​สิบ​เอ็ด | หนึ่ง​พัน​สอง​ร้อย​สาม​สิบ​สี่ | ลบ​ห้า | หนึ่ง​จุด​ห้า | สาม​จุด​สองห้า | ศูนย์​จุด​ศูนย์ห้า | สอง​พัน​ยี่​สิบ​สี่​จุด​เจ็ดห้า
vi: hai mươi mốt | một nghìn hai trăm ba mươi tư | âm năm | một phẩy năm | ba phẩy hai năm | không phẩy không năm | hai nghìn không trăm hai mươi tư phẩy bảy năm
nb: tjue­én | tusen to hundre og tretti­fire | minus fem | én komma fem | tre komma to fem | null komma null fem | to tusen og tjue­fire komma sju fem
sw: ishirini na moja | elfu moja, mia mbili na thelathini na nne | kasoro tano | moja nukta tano | tatu nukta mbili tano | sifuri nukta sifuri tano | elfu mbili, ishirini na nne nukta saba tano
ur: 21 | 1,234 | −5 | 1.5 | 3.2 | 0 | 2,024.8
not a number | minus infinity | 9,223,372,036,854,775,807
%spellout-numbering
%spellout-numbering-year;%spellout-numbering;%spellout-numbering-verbose;%spellout-cardinal;%spellout-cardinal-verbose;%spellout-ordinal;%spellout-ordinal-verbose;
bool(true)
%spellout-ordinal: one thousand nine hundred ninety-ninth / two thousand fifth / %spellout-ordinal
bool(true)
%spellout-numbering-year: nineteen ninety-nine / two thousand five / %spellout-numbering-year
bool(true)
%spellout-cardinal-verbose: one thousand nine hundred and ninety-nine / two thousand and five / %spellout-cardinal-verbose
bool(false)
%bogus: one thousand nine hundred ninety-nine / two thousand five / %spellout-numbering
bool(false)
%%2d-year: one thousand nine hundred ninety-nine / two thousand five / %spellout-numbering
%digits-ordinal-masculine;%digits-ordinal-feminine;%digits-ordinal-masculine-plural;%digits-ordinal-feminine-plural;%digits-ordinal;
1re 2e
een %b %a;%b;
