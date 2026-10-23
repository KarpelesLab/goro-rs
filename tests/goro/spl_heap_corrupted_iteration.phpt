--TEST--
Iterating a corrupted heap throws when reading or advancing
--FILE--
<?php
class H extends SplMinHeap{public $n=0;protected function compare($a,$b):int{if($this->n++>3)throw new Exception("boom");return parent::compare($a,$b);}}
$h=new H; try{foreach([1,2,3,4,5,6] as $x)$h->insert($x);}catch(Exception $e){echo $e->getMessage(),"\n";}
var_dump($h->isCorrupted(), count($h));
try{foreach($h as $v)echo $v,"\n";}catch(Throwable $e){echo get_class($e),": ",$e->getMessage(),"\n";}
var_dump(count($h));
try{ var_dump(iterator_to_array($h)); }catch(Throwable $e){echo get_class($e),": ",$e->getMessage(),"\n";}
try{ var_dump(iterator_count($h)); }catch(Throwable $e){echo get_class($e),": ",$e->getMessage(),"\n";}
try{ foreach (new IteratorIterator($h) as $v) echo $v, "\n"; }catch(Throwable $e){echo get_class($e),": ",$e->getMessage(),"\n";}
try{ var_dump($h->valid()); var_dump($h->current()); }catch(Throwable $e){echo get_class($e),": ",$e->getMessage(),"\n";}
try{ var_dump($h->key()); $h->next(); var_dump(count($h)); }catch(Throwable $e){echo get_class($e),": ",$e->getMessage(),"\n";}
try{ $h->rewind(); echo "rw\n"; }catch(Throwable $e){echo get_class($e),": ",$e->getMessage(),"\n";}
try{ foreach($h as $k => $v) { echo "$k\n"; } }catch(Throwable $e){echo get_class($e),": ",$e->getMessage(),"\n";}
$h->recoverFromCorruption();
try { foreach($h as $k => $v) { echo "$k=>$v\n"; } } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
// priority queue
class Q extends SplPriorityQueue{public $n=0;function compare($a,$b):int{if($this->n++>3)throw new Exception("boom");return parent::compare($a,$b);}}
$q=new Q; try{foreach([1,2,3,4,5,6] as $x)$q->insert($x,$x);}catch(Exception $e){echo $e->getMessage(),"\n";}
try{foreach($q as $v)echo $v,"\n";}catch(Throwable $e){echo get_class($e),": ",$e->getMessage(),"\n";}
var_dump(count($q));
// a subclass overriding current() iterates through its methods
class H2 extends SplMinHeap{public $n=0;protected function compare($a,$b):int{if($this->n++>3)throw new Exception("boom");return parent::compare($a,$b);} function current(): mixed { return parent::current(); }}
$h=new H2; try{foreach([1,2,3,4,5,6] as $x)$h->insert($x);}catch(Exception $e){echo $e->getMessage(),"\n";}
try{foreach($h as $v)echo $v,"\n";}catch(Throwable $e){echo get_class($e),": ",$e->getMessage(),"\n";}
// traces of compare() called by heap operations
$h=new H; $h->n = 100; try { $h->insert(1); $h->insert(2); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
$h=new H; $h->insert(1); $h->insert(2); $h->insert(3); $h->n = 100; try { $h->extract(); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
$h=new H; $h->insert(1); $h->insert(2); $h->insert(3); $h->n = 100; try { foreach ($h as $v) {} } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
$h=new H; $h->insert(1); $h->insert(2); $h->insert(3); $h->n = 100; try { iterator_to_array($h); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
--EXPECTF--
boom
bool(true)
int(6)
RuntimeException: Heap is corrupted, heap properties are no longer ensured.
int(6)
RuntimeException: Heap is corrupted, heap properties are no longer ensured.
RuntimeException: Heap is corrupted, heap properties are no longer ensured.
RuntimeException: Heap is corrupted, heap properties are no longer ensured.
bool(true)
int(1)
int(5)
Exception: boom
rw
RuntimeException: Heap is corrupted, heap properties are no longer ensured.
4=>6
#0 %s(15): H->compare(3, 2)
#1 {main}
boom
RuntimeException: Heap is corrupted, heap properties are no longer ensured.
int(5)
boom
1
Exception: boom
#0 [internal function]: H->compare(1, 2)
#1 %s(26): SplHeap->insert(2)
#2 {main}
#0 [internal function]: H->compare(3, 2)
#1 %s(27): SplHeap->extract()
#2 {main}
#0 %s(28): H->compare(3, 2)
#1 {main}
#0 [internal function]: H->compare(3, 2)
#1 %s(29): iterator_to_array(Object(H))
#2 {main}
