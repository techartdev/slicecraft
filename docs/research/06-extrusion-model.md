# R6 — Extrusion model: how E follows width, height and speed

> **Research note 6.** Question: *how much filament does the reference push per
> millimetre of path, and does SliceCraft match it?* Method: G-code written by
> Simplify3D 4.0.x and 5.1.2 for the operator's jobs. Each extrusion move's E per
> mm was compared with the line width and height in the file's `; tool H… W…`
> comments, and with the multiplier and filament diameter in its settings summary.

## 1. Evidence

| Job (version) | Width × height | Multiplier, filament | Measured E/mm | w·h / A · m | rounded bead |
|---|---|---|---|---|---|
| crown (4.0.1) | 0.40 × 0.20 | 1.00, 1.75 | 0.03325 | **0.03326** | 0.02969 |
| baby rattle (4.0.0), first layer | 0.72 × 0.28 | 1.02, 1.73 | 0.08748 | **0.08748** | 0.08018 |
| baby rattle (4.0.0) | 0.48 × 0.28 | 1.02, 1.73 | 0.05832 | **0.05832** | 0.05102 |
| fence (5.1.2) | 0.48 × 0.28 | 1.01, 1.73 | 0.05775 | **0.05775** | 0.05052 |

The values are medians over thousands of moves longer than 2 mm. A is the
filament's cross-section, π·d²/4.

Also observed:
- The first layer is printed at the profile's first-layer width percentage of
  the normal width (150 % gives 0.72 from 0.48).
- E does not depend on speed.

## 2. Conclusion

The reference's volume per millimetre is a **rectangle**: width × height ×
multiplier. SliceCraft used a rounded-sides bead, `(w − h)·h + π·h²/4`, which gives
10–14 % less at common sizes. A profile tuned in the reference (its multiplier
compensating for the real bead) would therefore under-extrude in SliceCraft.

## 3. What changed

- A `bead_shape` setting. The default **rectangle** matches the reference, and
  **rounded sides** keeps the old model.
- `first_layer_width_percent` is live. With a raft, the raft is the first layer and
  the part's first layer prints at the normal width.
- Crown job after the change: outer perimeter 1641 mm against the reference's
  1642 mm on the same 49.3 m path.

## 4. Open questions

1. Gap-fill lines in the reference change width with the gap. Ours use the normal
   width (see [gaps](../gaps.md)).
2. Coasting and wipe change E at path ends; they are not measured yet.
