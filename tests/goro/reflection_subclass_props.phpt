--TEST--
Properties of user classes extending Reflection classes (their objects' own state kept apart)
--FILE--
<?php
class RC extends ReflectionClass {
    private ?object $inner = null;
    public int $hits = 0;
    protected array $cache = [];
    public function __construct(string $c) { parent::__construct($c); $this->inner = new stdClass; $this->inner->n = 2; }
    public function inner(): ?object { $this->hits++; return $this->inner; }
    public function cached(string $k): int { return $this->cache[$k] ??= strlen($k) + $this->inner->n; }
}
class RM extends ReflectionMethod {
    public $tag = 'm';
}
class RP extends ReflectionProperty {
    private $note;
    public function setNote($n) { $this->note = $n; return $this; }
    public function note() { return $this->note; }
}
class Sub { public $x = 1; private function hidden() {} }
function run() {
    $r = new RC('Sub');
    $s = 0;
    for ($i = 0; $i < 50; $i++) {
        $s += $r->inner()->n + $r->cached("k" . ($i % 3));
    }
    echo $s, " ", $r->hits, " ", $r->name, " ", $r->getName(), "\n";
    try {
        $r->hits = "many";
    } catch (TypeError $e) {
        echo get_class($e), ": ", $e->getMessage(), "\n";
    }
    echo isset($r->hits) ? "set" : "unset", " ", isset($r->nope) ? "set" : "unset", "\n";
    $m = new RM('Sub', 'hidden');
    echo $m->tag, " ", $m->name, " ", $m->class, " ", $m->isPrivate() ? "private" : "public", "\n";
    $m->tag = 'n';
    echo $m->tag, "\n";
    $p = (new RP('Sub', 'x'))->setNote('noted');
    for ($i = 0; $i < 3; $i++) {
        echo $p->note(), " ", $p->name, " ", $p->getValue(new Sub), "\n";
    }
    try {
        $r->name = 'Other';
    } catch (ReflectionException $e) {
        echo get_class($e), ": ", $e->getMessage(), "\n";
    }
    var_dump(count((array) $r) > 0);
}
run();
?>
--EXPECT--
300 50 Sub Sub
TypeError: Cannot assign string to property RC::$hits of type int
set unset
m hidden Sub private
n
noted x 1
noted x 1
noted x 1
ReflectionException: Cannot set read-only property RC::$name
bool(true)
