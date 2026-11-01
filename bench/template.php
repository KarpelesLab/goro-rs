<?php
// Mixed workload: building rows and rendering an HTML table with escaping.
function e($s) { return htmlspecialchars((string)$s, ENT_QUOTES, 'UTF-8'); }
class Row {
    public function __construct(public int $id, public string $name, public float $price, public array $tags) {}
    public function priceLabel(): string { return number_format($this->price, 2) . ' EUR'; }
}
function render(array $rows, array $opts): string {
    ob_start();
    echo "<table class=\"", e($opts['class']), "\">\n";
    foreach ($rows as $i => $row) {
        $cls = $i % 2 ? 'odd' : 'even';
        echo "<tr class=\"$cls\"><td>{$row->id}</td><td>", e($row->name), "</td><td>", $row->priceLabel(), "</td><td>";
        echo implode(', ', array_map('e', $row->tags)), "</td></tr>\n";
    }
    echo "</table>\n";
    return ob_get_clean();
}
$t = hrtime(true);
$check = 0;
for ($round = 0; $round < 200; $round++) {
    $rows = [];
    for ($i = 0; $i < 2000; $i++) {
        $rows[] = new Row($i, "Product <$i> & \"co\"", $i * 1.37, ['t' . ($i % 5), 'cat' . ($i % 3), "x&y"]);
    }
    $html = render($rows, ['class' => 'list']);
    $data = ['title' => 'Report', 'count' => count($rows), 'items' => array_slice($rows, 0, 10)];
    $out = strtr("<h1>{title}</h1><p>{count} items</p>", ['{title}' => e($data['title']), '{count}' => $data['count']]);
    $check += strlen($html) + strlen($out) + substr_count($html, '<tr');
}
$ms = (hrtime(true) - $t) / 1e6;
printf("time_ms=%.3f check=%s\n", $ms, $check);
