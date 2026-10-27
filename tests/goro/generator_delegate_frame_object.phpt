--TEST--
debug_backtrace(): object of a generator method frame that just started a yield from; line of a closure generator yielded from
--FILE--
<?php
$cbx = function ($x) {
    foreach (debug_backtrace(DEBUG_BACKTRACE_PROVIDE_OBJECT | DEBUG_BACKTRACE_IGNORE_ARGS) as $i => $f) {
        echo $i, ' ', $f['class'] ?? '', ' ', $i ? $f['function'] : 'cb', ' ', $f['line'] ?? '-', ' ', isset($f['object']) ? get_class($f['object']) : '', "\n";
    }
    exit;
};
abstract class B {
    public function render(array $context): string
    {
        $content = '';
        foreach ($this->yield($context) as $data) {
            $content .= $data;
        }

        return $content;
    }
    public function yield(array $context, array $blocks = []): iterable
    {
        try {
            yield from $this->doDisplay($context, $blocks);
        } catch (\Throwable $e) {
            throw $e;
        }
    }
    abstract protected function doDisplay(array $context, array $blocks = []): iterable;
}
class T_61b68a5408812bfa674874c5d4cb4a99 extends B {
    private array $macros = [];
    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        // line 1
        yield "<div>
";
        // line 2
        $_v0 = new \ArrayIterator(($context["items"] ?? null));
        yield from ($_v1 = function ($iterator, &$context, $blocks, $recurseFunc, $depth) {
            $macros = $this->macros;
            $parent = $context;
            foreach ($iterator as $context["_key"] => $context["item"]) {
                // line 3
                yield (string) $GLOBALS["cbx"]($context["item"]);
                yield "
";
            }
            unset($context['_key'], $context['item']);
            $context = array_intersect_key($context, $parent) + $parent;
            return; yield;
        })($_v0, $context, $blocks, $_v1, 0);
        // line 5
        yield "</div>
";
        return; yield;
    }
}
$t = new T_61b68a5408812bfa674874c5d4cb4a99; $t->render(["items" => [1]]);

?>
--EXPECT--
0  cb 43 
1 T_61b68a5408812bfa674874c5d4cb4a99 {closure:T_61b68a5408812bfa674874c5d4cb4a99::doDisplay():38} 50 T_61b68a5408812bfa674874c5d4cb4a99
2 T_61b68a5408812bfa674874c5d4cb4a99 doDisplay 21 T_61b68a5408812bfa674874c5d4cb4a99
3 B yield 12 T_61b68a5408812bfa674874c5d4cb4a99
4 B render 57 T_61b68a5408812bfa674874c5d4cb4a99
