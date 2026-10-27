--TEST--
A closure's generator resolves static as the closure does, whoever resumes it
--FILE--
<?php
class Each {
    public function __construct(private $it) {}
    public function run() {
        $out = [];
        $this->it->rewind();
        while ($this->it->valid()) {
            $out[] = $this->it->current();
            $this->it->next();
        }
        return $out;
    }
}
class P {
    public function pool() {
        $items = [new P, new Q, 1];
        $g = static function () use ($items) {
            foreach ($items as $k => $item) {
                yield $k => [$item instanceof static, static::class, self::class];
            }
        };
        var_dump((new Each($g()))->run());
        $h = function () use ($items) {
            foreach ($items as $k => $item) {
                yield $k => [$item instanceof static, static::class];
            }
        };
        var_dump((new Each($h()))->run());
    }
}
class Q extends P {}
(new P)->pool();
(new Q)->pool();
class S { static function m() { return (static function () { yield static::class; })(); } }
class T extends S {}
$gen = T::m();
var_dump((new Each($gen))->run());
$b = Closure::bind(function () { yield static::class; }, null, T::class);
var_dump((new Each($b()))->run());

?>
--EXPECT--
array(3) {
  [0]=>
  array(3) {
    [0]=>
    bool(true)
    [1]=>
    string(1) "P"
    [2]=>
    string(1) "P"
  }
  [1]=>
  array(3) {
    [0]=>
    bool(true)
    [1]=>
    string(1) "P"
    [2]=>
    string(1) "P"
  }
  [2]=>
  array(3) {
    [0]=>
    bool(false)
    [1]=>
    string(1) "P"
    [2]=>
    string(1) "P"
  }
}
array(3) {
  [0]=>
  array(2) {
    [0]=>
    bool(true)
    [1]=>
    string(1) "P"
  }
  [1]=>
  array(2) {
    [0]=>
    bool(true)
    [1]=>
    string(1) "P"
  }
  [2]=>
  array(2) {
    [0]=>
    bool(false)
    [1]=>
    string(1) "P"
  }
}
array(3) {
  [0]=>
  array(3) {
    [0]=>
    bool(false)
    [1]=>
    string(1) "Q"
    [2]=>
    string(1) "P"
  }
  [1]=>
  array(3) {
    [0]=>
    bool(true)
    [1]=>
    string(1) "Q"
    [2]=>
    string(1) "P"
  }
  [2]=>
  array(3) {
    [0]=>
    bool(false)
    [1]=>
    string(1) "Q"
    [2]=>
    string(1) "P"
  }
}
array(3) {
  [0]=>
  array(2) {
    [0]=>
    bool(false)
    [1]=>
    string(1) "Q"
  }
  [1]=>
  array(2) {
    [0]=>
    bool(true)
    [1]=>
    string(1) "Q"
  }
  [2]=>
  array(2) {
    [0]=>
    bool(false)
    [1]=>
    string(1) "Q"
  }
}
array(1) {
  [0]=>
  string(1) "T"
}
array(1) {
  [0]=>
  string(1) "T"
}
