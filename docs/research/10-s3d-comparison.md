# R10 — Side by side with Simplify3D on real jobs

**Question.** On real jobs, does SliceCraft lay down the same plastic as Simplify3D,
in no more time and with no more stops, when both slice the same model with the
same settings?

**Method.** The operator's own folder of finished jobs (kept outside the repository)
holds Simplify3D 4.0 G-code files next to the STL they were sliced from. Simplify3D
writes every setting it used into a header at the top of the G-code; SliceCraft now
reads that header as a profile (`io::fff::import_s3d_gcode`), so each job is
re-sliced with exactly the settings Simplify3D used. Jobs where the model had been
turned or scaled in Simplify3D were matched by trying the model as is, flipped and
turned 90°, and keeping the orientation whose height, footprint and layer-by-layer
path lengths match; jobs no orientation matched (scaled, or turned by another angle)
were left out. Both files are then measured by the same script: extruded filament,
retractions, layer count, and time by SliceCraft's own estimator run over both files
(one yardstick for both; neither is a measured print time).

The jobs are flat decorative pieces (cake toppers, lettering plates), 6 to 107
layers, 0.3 mm layers, 100 % infill, two walls. Names are left out; they are the
operator's customers' jobs.

## Results (10 jobs that matched)

| Job | Layers | Filament, mm (S3D → SliceCraft) | Retractions | Time, s (same estimator) |
|---|---|---|---|---|
| 1 | 7 = 7 | 5789 → 5756 (−0.6 %) | 194 → 297 | 2388 → 2284 (−4.4 %) |
| 2 | 7 = 7 | 6271 → 6228 (−0.7 %) | 280 → 351 | 2639 → 2509 (−4.9 %) |
| 3 | 7 = 7 | 3403 → 3375 (−0.8 %) | 448 → 516 | 1674 → 1542 (−7.9 %) |
| 4 | 7 = 7 | 4252 → 4225 (−0.6 %) | 572 → 561 | 2067 → 1931 (−6.6 %) |
| 5 | 107 = 107 | 2186 → 2177 (−0.4 %) | 1146 → 1185 | 1408 → 1292 (−8.2 %) |
| 6 | 90 = 90 | 2835 → 2794 (−1.4 %) | 936 → 950 | 1584 → 1410 (−11.0 %) |
| 7 | 7 = 7 | 2953 → 2917 (−1.2 %) | 1023 → 959 | 1624 → 1472 (−9.4 %) |
| 8 | 8 = 8 | 4652 → 4483 (−3.6 %) | 127 → 140 | 1939 → 1774 (−8.5 %) |
| 9 | 6 = 6 | 3004 → 2995 (−0.3 %) | 1768 → 1682 | 2026 → 1739 (−14.2 %) |
| 10 | 7 = 7 | 2744 → 2711 (−1.2 %) | 409 → 395 | 1407 → 1275 (−9.4 %) |

Earlier (R5, R6): on a 350-layer crown with support, support filament 676 vs 682 mm,
outer-wall filament 1641 vs 1642 mm, retractions 2606 vs 4384.

## What the comparison found and fixed

Running it first showed SliceCraft 5–15 % short on filament. Three causes, all fixed:

1. **Thin walls were imported wrongly.** Simplify3D's "internal thin wall" type means
   single lines in the gaps between walls and infill; it was mapped to SliceCraft's
   thin-feature mode, which leaves those gaps empty. Now mapped to gap fill.
2. **Dense infill was not linked.** Simplify3D joins neighbouring fill lines into
   zigzags when they are close (100 % and solid fill: one travel per ~90 segments;
   10 % fill: nearly every line separate). SliceCraft's minimum infill length then
   dropped short single lines in narrow strokes. Lines within three line widths are
   now linked, and dense lines stop half a width short of the edge so the links add
   no material. Per-layer fill length now matches within 2 %.
3. **Walls were printed feature by feature across the whole layer.** Simplify3D
   prints each outline's inner and outer wall together before moving on. SliceCraft
   now does the same, and joins gap-fill pieces that meet end to end.
4. **The nearest next path was taken even when reaching it meant leaving the part.**
   Among the few nearest candidates, one reachable without crossing an outline (no
   retraction) now wins unless it is more than 10 mm farther.

## Evidence and inference

- **Evidence:** layer counts equal on every job; filament within 0.3–3.6 %; by the
  same estimator SliceCraft's paths take 4–14 % less time; retractions 6 % fewer to
  53 % more (fewer on 4 of 10 jobs, median about +4 %).
- **Inference:** equal filament with equal settings means the extrusion model and
  the fill areas agree. The time difference comes from fewer and shorter travels; it
  is an estimate, not a measured print. More retractions on lettering come mainly from
  gap fill that crosses letter holes and from the start of each outline; whether that
  shows as stringing is unknown until printed.

## Open questions

1. Print the same job from both files and compare: surface, stringing, dimensions,
   and the printer's own print time.
2. Retractions on lettering: route gap-fill travel inside the part (combing) or print
   gap fill with its outline's walls, as Simplify3D appears to.
3. Jobs with low infill and support were not in the matched set beyond the crown;
   find more pairs of that kind.
