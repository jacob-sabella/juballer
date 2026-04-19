#!/bin/bash
# High-quality procedural marker sprite generator. Uses ImageMagick's radial
# gradients + Gaussian blur for soft glows instead of hard geometric shapes
# (which looked like crude programmer-art in the first iteration).
#
# Output: assets/markers/tap/juballer_default/
#   approach.png  (16 frames, 4×4 grid, 150×150 each, 1s lead-in @ 16fps)
#   perfect/great/good/poor.png  (9 frames, 5×2, snappy 30fps bursts)
#   miss.png      (9 frames, 3×3, 24fps fade)
#   marker.json   (per-anim fps)
#
# Re-run after editing.

set -euo pipefail
SIZE=150
OUT=assets/markers/tap/juballer_default
mkdir -p "$OUT"
TMP=$(mktemp -d)
trap "rm -rf $TMP" EXIT

# Build one glow-ring frame: soft outer glow + crisp inner ring at radius `r`.
# Arg order: outfile, ring_radius_px, ring_width_px, R, G, B, alpha_core, glow_alpha
glow_ring() {
    out="$1" rr="$2" rw="$3" R="$4" G="$5" B="$6" CA="$7" GA="$8"
    center=$(( SIZE / 2 ))
    # Ring stroke + thick blurred copy for the glow. Composite together.
    magick -size ${SIZE}x${SIZE} xc:none \
        -stroke "rgba($R,$G,$B,$GA)" -strokewidth $(( rw + 14 )) -fill none \
        -draw "circle $center,$center $((center + rr)),$center" \
        -blur 0x8 \
        \( -size ${SIZE}x${SIZE} xc:none \
           -stroke "rgba($R,$G,$B,$CA)" -strokewidth $rw -fill none \
           -draw "circle $center,$center $((center + rr)),$center" \) \
        -compose screen -composite \
        "$out"
}

# ── Approach: 16 frames. Big faint ring closes to a small bright core. ──
for i in $(seq 0 15); do
    t=$(awk "BEGIN{print $i / 15}")
    # Radius shrinks ~0.44 → 0.18 of tile (px).
    rr=$(awk "BEGIN{printf \"%d\", ($SIZE * 0.44) - ($SIZE * 0.26) * $t}")
    # Width grows slightly so close-in ring reads bold.
    rw=$(awk "BEGIN{printf \"%d\", 3 + 4 * $t}")
    ca=$(awk "BEGIN{printf \"%.2f\", 0.55 + 0.45 * $t}")
    ga=$(awk "BEGIN{printf \"%.2f\", 0.15 + 0.40 * $t}")
    glow_ring "$TMP/approach_$i.png" $rr $rw 220 230 255 $ca $ga
done
magick montage -background none "$TMP"/approach_{0..15}.png \
    -tile 4x4 -geometry ${SIZE}x${SIZE}+0+0 "$OUT/approach.png"

# ── Grade burst: starburst disc that expands + fades, per-grade color. ──
# Args: outfile, R, G, B, ray_count
burst() {
    out="$1" R="$2" G="$3" B="$4" RAYS="$5"
    for i in $(seq 0 8); do
        t=$(awk "BEGIN{print $i / 8}")
        local center=$(( SIZE / 2 ))
        # Soft expanding glow disc.
        local r_px=$(awk "BEGIN{printf \"%d\", $SIZE * (0.12 + 0.38 * $t)}")
        local alpha=$(awk "BEGIN{printf \"%.2f\", 0.95 * (1.0 - $t * $t)}")
        local frame="$TMP/burst_$i.png"
        # Filled disc + heavy blur → glowy halo.
        magick -size ${SIZE}x${SIZE} xc:none \
            -fill "rgba($R,$G,$B,$alpha)" -stroke none \
            -draw "circle $center,$center $((center + r_px)),$center" \
            -blur 0x$(awk "BEGIN{printf \"%d\", 4 + 12 * $t}") \
            "$frame.glow.png"
        # Sharp inner disc, smaller, brighter.
        local r_inner=$(awk "BEGIN{printf \"%d\", $r_px / 2}")
        local core_a=$(awk "BEGIN{printf \"%.2f\", (0.85 * (1.0 - $t))}")
        magick -size ${SIZE}x${SIZE} xc:none \
            -fill "rgba(255,255,255,$core_a)" -stroke none \
            -draw "circle $center,$center $((center + r_inner)),$center" \
            -blur 0x2 \
            "$frame.core.png"
        # Rays — thin lines from center, fade with t.
        magick -size ${SIZE}x${SIZE} xc:none "$frame.rays.png"
        local ray_a=$(awk "BEGIN{printf \"%.2f\", 0.75 * (1.0 - $t)}")
        for j in $(seq 0 $((RAYS - 1))); do
            local ang=$(awk "BEGIN{print $j * 6.2831853 / $RAYS}")
            local dx=$(awk "BEGIN{printf \"%d\", $r_px * cos($ang) * 1.1}")
            local dy=$(awk "BEGIN{printf \"%d\", $r_px * sin($ang) * 1.1}")
            magick "$frame.rays.png" \
                -stroke "rgba($R,$G,$B,$ray_a)" -strokewidth 3 \
                -draw "line $center,$center $((center + dx)),$((center + dy))" \
                "$frame.rays.png"
        done
        magick "$frame.rays.png" -blur 0x2 "$frame.rays.png"
        # Composite: glow + rays + core on top.
        magick "$frame.glow.png" "$frame.rays.png" -compose screen -composite \
               "$frame.core.png" -compose screen -composite "$frame"
    done
    magick montage -background none "$TMP"/burst_{0..8}.png \
        -tile 5x2 -geometry ${SIZE}x${SIZE}+0+0 "$out"
}

burst "$OUT/perfect.png"  80 235 140 12
burst "$OUT/great.png"   240 220  90 10
burst "$OUT/good.png"    245 165  70  8
burst "$OUT/poor.png"    230  95  95  6

# ── Miss: dark red pulse + X crack, no rays. 3×3 grid. ──
for i in $(seq 0 8); do
    t=$(awk "BEGIN{print $i / 8}")
    center=$(( SIZE / 2 ))
    alpha=$(awk "BEGIN{printf \"%.2f\", 0.85 * (1.0 - $t)}")
    rr=$(awk "BEGIN{printf \"%d\", $SIZE * (0.22 - 0.08 * $t)}")
    # Soft red fill.
    magick -size ${SIZE}x${SIZE} xc:none \
        -fill "rgba(180,40,40,$alpha)" -stroke none \
        -draw "circle $center,$center $((center + rr)),$center" \
        -blur 0x6 \
        "$TMP/miss_$i.bg.png"
    # Diagonal crack lines (X).
    crack_a=$(awk "BEGIN{printf \"%.2f\", 0.80 * (1.0 - $t * 0.7)}")
    magick -size ${SIZE}x${SIZE} xc:none \
        -stroke "rgba(230,120,120,$crack_a)" -strokewidth 4 -fill none \
        -draw "line $((center - rr - 6)),$((center - rr - 6)) $((center + rr + 6)),$((center + rr + 6))" \
        -draw "line $((center - rr - 6)),$((center + rr + 6)) $((center + rr + 6)),$((center - rr - 6))" \
        -blur 0x1 \
        "$TMP/miss_$i.x.png"
    magick "$TMP/miss_$i.bg.png" "$TMP/miss_$i.x.png" \
        -compose screen -composite "$TMP/miss_$i.png"
done
magick montage -background none "$TMP"/miss_{0..8}.png \
    -tile 3x3 -geometry ${SIZE}x${SIZE}+0+0 "$OUT/miss.png"

# ── marker.json ──
cat > "$OUT/marker.json" <<JSON
{
    "name": "juballer default",
    "size": $SIZE,
    "fps": 30,
    "approach": { "sprite_sheet": "approach.png", "count": 16, "columns": 4, "rows": 4, "fps": 16 },
    "perfect":  { "sprite_sheet": "perfect.png",  "count": 9,  "columns": 5, "rows": 2, "fps": 30 },
    "great":    { "sprite_sheet": "great.png",    "count": 9,  "columns": 5, "rows": 2, "fps": 30 },
    "good":     { "sprite_sheet": "good.png",     "count": 9,  "columns": 5, "rows": 2, "fps": 30 },
    "poor":     { "sprite_sheet": "poor.png",     "count": 9,  "columns": 5, "rows": 2, "fps": 30 },
    "miss":     { "sprite_sheet": "miss.png",     "count": 9,  "columns": 3, "rows": 3, "fps": 24 }
}
JSON

cat > "$OUT/SOURCE.txt" <<EOF
Procedurally generated via scripts/generate_markers.sh (ImageMagick).
Re-run that script to regenerate. No third-party assets.
EOF

echo "Generated at $OUT:"
ls -la "$OUT"
