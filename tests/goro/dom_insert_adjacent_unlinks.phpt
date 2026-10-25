--TEST--
insertAdjacentElement() unlinks the node before validating the insertion
--FILE--
<?php
foreach (["legacy", "modern"] as $k) {
foreach (["beforebegin","afterbegin","beforeend","afterend"] as $w) {
    foreach ([0, 1, 2, 3] as $case) {
        if ($k == 'legacy') {
            $d = new DOMDocument;
            $d->loadXML('<top><r><x/><y/></r></top>');
        } else {
            $d = Dom\XMLDocument::createFromString('<top><r><x/><y/></r></top>');
        }
        $r = $d->documentElement->firstChild;
        $x = $r->firstChild; $y = $r->lastChild;
        [$a, $b] = [[$x, $r], [$r, $d->documentElement], [$x, $y], [$y, $x]][$case];
        echo "$w $case: ";
        try {
            $wh = $k == 'legacy' ? $w : constant('Dom\AdjacentPosition::' . ['beforebegin' => 'BeforeBegin', 'afterbegin' => 'AfterBegin', 'beforeend' => 'BeforeEnd', 'afterend' => 'AfterEnd'][$w]);
            var_dump($a->insertAdjacentElement($wh, $b)?->nodeName);
        } catch (Throwable $e) { echo get_class($e), " ", $e->getMessage(), "\n"; }
        echo $d->saveXML(), "\n";
    }
}
}
--EXPECT--
beforebegin 0: DOMException Hierarchy Request Error
<?xml version="1.0"?>
<top/>

beforebegin 1: DOMException Hierarchy Request Error
<?xml version="1.0"?>

beforebegin 2: string(1) "y"
<?xml version="1.0"?>
<top><r><y/><x/></r></top>

beforebegin 3: string(1) "x"
<?xml version="1.0"?>
<top><r><x/><y/></r></top>

afterbegin 0: DOMException Hierarchy Request Error
<?xml version="1.0"?>
<top/>

afterbegin 1: DOMException Hierarchy Request Error
<?xml version="1.0"?>

afterbegin 2: string(1) "y"
<?xml version="1.0"?>
<top><r><x><y/></x></r></top>

afterbegin 3: string(1) "x"
<?xml version="1.0"?>
<top><r><y><x/></y></r></top>

beforeend 0: DOMException Hierarchy Request Error
<?xml version="1.0"?>
<top/>

beforeend 1: DOMException Hierarchy Request Error
<?xml version="1.0"?>

beforeend 2: string(1) "y"
<?xml version="1.0"?>
<top><r><x><y/></x></r></top>

beforeend 3: string(1) "x"
<?xml version="1.0"?>
<top><r><y><x/></y></r></top>

afterend 0: DOMException Hierarchy Request Error
<?xml version="1.0"?>
<top/>

afterend 1: DOMException Hierarchy Request Error
<?xml version="1.0"?>

afterend 2: string(1) "y"
<?xml version="1.0"?>
<top><r><x/><y/></r></top>

afterend 3: string(1) "x"
<?xml version="1.0"?>
<top><r><y/><x/></r></top>

beforebegin 0: DOMException Hierarchy Request Error
<?xml version="1.0" encoding="UTF-8"?>
<top/>
beforebegin 1: DOMException Hierarchy Request Error
<?xml version="1.0" encoding="UTF-8"?>

beforebegin 2: string(1) "y"
<?xml version="1.0" encoding="UTF-8"?>
<top><r><y/><x/></r></top>
beforebegin 3: string(1) "x"
<?xml version="1.0" encoding="UTF-8"?>
<top><r><x/><y/></r></top>
afterbegin 0: DOMException Hierarchy Request Error
<?xml version="1.0" encoding="UTF-8"?>
<top/>
afterbegin 1: DOMException Hierarchy Request Error
<?xml version="1.0" encoding="UTF-8"?>

afterbegin 2: string(1) "y"
<?xml version="1.0" encoding="UTF-8"?>
<top><r><x><y/></x></r></top>
afterbegin 3: string(1) "x"
<?xml version="1.0" encoding="UTF-8"?>
<top><r><y><x/></y></r></top>
beforeend 0: DOMException Hierarchy Request Error
<?xml version="1.0" encoding="UTF-8"?>
<top/>
beforeend 1: DOMException Hierarchy Request Error
<?xml version="1.0" encoding="UTF-8"?>

beforeend 2: string(1) "y"
<?xml version="1.0" encoding="UTF-8"?>
<top><r><x><y/></x></r></top>
beforeend 3: string(1) "x"
<?xml version="1.0" encoding="UTF-8"?>
<top><r><y><x/></y></r></top>
afterend 0: DOMException Hierarchy Request Error
<?xml version="1.0" encoding="UTF-8"?>
<top/>
afterend 1: DOMException Hierarchy Request Error
<?xml version="1.0" encoding="UTF-8"?>

afterend 2: string(1) "y"
<?xml version="1.0" encoding="UTF-8"?>
<top><r><x/><y/></r></top>
afterend 3: string(1) "x"
<?xml version="1.0" encoding="UTF-8"?>
<top><r><y/><x/></r></top>
