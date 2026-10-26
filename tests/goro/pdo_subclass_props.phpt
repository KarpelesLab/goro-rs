--TEST--
Properties of user classes extending PDO and PDOStatement
--EXTENSIONS--
pdo_sqlite
--FILE--
<?php
class Db extends PDO {
    public $queries = 0;
    private $log = [];
    protected $name = 'db';
    public $stmtClass;
    function q($sql) {
        $this->queries++;
        $this->log[] = $sql;
        $this->queries += 0;
        $s = $this->query($sql);
        return $s;
    }
    function log() { return [$this->name, $this->log, isset($this->log[0]), isset($this->nope)]; }
}
class St extends PDOStatement {
    public $seen = 0;
    protected function __construct() {}
}
$db = new Db('sqlite::memory:');
$db->setAttribute(PDO::ATTR_STATEMENT_CLASS, ['St', []]);
$db->exec('create table t (a int)');
for ($i = 0; $i < 3; $i++) {
    $db->exec("insert into t values ($i)");
    $s = $db->q('select a from t order by a');
    $s->seen += 1;
    $s->extra = $i;
    echo get_class($s), ' ', $s->queryString, ' ', $s->seen, ' ', $s->extra, ' ', json_encode($s->fetchAll(PDO::FETCH_COLUMN)), "\n";
}
echo $db->queries, ' ', json_encode($db->log()), "\n";
try {
    echo $db->name;
} catch (Error $e) {
    echo $e->getMessage(), "\n";
}
$db->dyn = 5;
$db->dyn += 2;
echo $db->dyn, "\n";
unset($db->queries);
echo isset($db->queries) ? 'set' : 'unset', "\n";
?>
--EXPECTF--

Deprecated: Creation of dynamic property St::$extra is deprecated in %s on line 27
St select a from t order by a 1 0 [0]

Deprecated: Creation of dynamic property St::$extra is deprecated in %s on line 27
St select a from t order by a 1 1 [0,1]

Deprecated: Creation of dynamic property St::$extra is deprecated in %s on line 27
St select a from t order by a 1 2 [0,1,2]
3 ["db",["select a from t order by a","select a from t order by a","select a from t order by a"],true,false]
Cannot access protected property Db::$name

Deprecated: Creation of dynamic property Db::$dyn is deprecated in %s on line 36
7
unset
