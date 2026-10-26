--TEST--
PhpToken subclasses without their own constructor get PhpToken::__construct(); (string) of a subclass
--FILE--
<?php
// nikic/php-parser: PhpParser\Token extends Internal\TokenPolyfill extends \PhpToken
// (the polyfill declared inside an if)
if (PHP_VERSION_ID >= 80000) { class TP extends PhpToken {} }
class Tok extends TP {
    public function getEndPos(): int { return $this->pos + \strlen($this->text); }
}
$t = new Tok(0, "\n", 1, 0);
var_dump($t->line, json_encode($t->text), $t->getEndPos());
$toks = Tok::tokenize("<?php echo 1;");
foreach ($toks as $x) echo get_class($x), ' ', $x->getTokenName(), ' ', json_encode((string) $x), "\n";
class TQ extends PhpToken {}
$c = 'TQ';
var_dump(new $c(1, 'b', 2, 3));
var_dump((string) new TQ(T_STRING, 'str'));

?>
--EXPECT--
int(1)
string(4) ""\n""
int(1)
Tok T_OPEN_TAG "<?php "
Tok T_ECHO "echo"
Tok T_WHITESPACE " "
Tok T_LNUMBER "1"
Tok ; ";"
object(TQ)#7 (4) {
  ["id"]=>
  int(1)
  ["text"]=>
  string(1) "b"
  ["line"]=>
  int(2)
  ["pos"]=>
  int(3)
}
string(3) "str"
