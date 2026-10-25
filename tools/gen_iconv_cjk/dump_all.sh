#!/bin/bash
# dump_all.sh PHP OUTDIR: the glibc dumps gen.py reads (a few minutes)
PHP=$1
OUT=$2
D="$PHP -n $(dirname "$0")/dump.php"
mkdir -p "$OUT"
for cs in JOHAB BIG5-HKSCS EUC-JISX0213 SHIFT_JISX0213; do
    ($D $cs D1 > "$OUT/$cs.d1"; $D $cs D2 > "$OUT/$cs.d2"; $D $cs E > "$OUT/$cs.e") &
done
$D JOHAB IGN > "$OUT/JOHAB.ign" &
$D EUC-JISX0213 D2 8f a1 > "$OUT/EUC-JISX0213.p2" &
wait
for p in 3 4 5 6 7 f; do
    $D EUC-TW D2 8ea$p a1 > "$OUT/EUC-TW.p$((16#$p))" &
done
$D EUC-TW E > "$OUT/EUC-TW.e" &
$D ISO-2022-CN-EXT D2 1b2429450e 21 > "$OUT/ISO-IR-165.d2" &
wait
