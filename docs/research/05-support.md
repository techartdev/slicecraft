# R5 — Support: what the reference does, and SliceCraft's design

> **Research note 5.** Question: *how does Simplify3D build automatic support, and
> what should SliceCraft build?* Method: G-code that Simplify3D 4.0.x wrote for
> the operator's own jobs with support on, read move by move against the settings
> summary in each file's header. The design in `crates/engine/src/support.rs` is our
> own; this note records what was observed so that differences are deliberate.

## 1. Evidence base

| Source | What it gave |
|---|---|
| 4 Simplify3D 4.0.x G-code files with `generateSupport,1` among the operator's 158; one (a crown, 350 layers) sits next to its STL | support moves per layer, spacing, directions, extent; a model to slice ourselves for comparison |

## 2. Observed (evidence)

On the crown job (support density 30 %, line width 0.4 mm, grid spacing 1.5 mm, 5 base
layers, overhang angle 45°, horizontal offset 0.2 mm, 1 separation layer above and
below):

- **Lines.** Support is printed as lines parallel to X (support angle 0), 1.33 mm
  apart. That is width / density = 0.4 / 0.30.
- **Base layers.** On the first layers the lines are 0.40 mm apart, which is solid.
- **Pillars.** Support comes as separate small blocks of about 3.1 × 2.7 mm. Each is
  printed as one zigzag: three X lines joined by short steps in Y. Their positions
  are consistent with a grid twice the configured spacing.
- **Extent.** Support appears on 305 of the 350 layers. Per layer the amount is
  steady (about 66 mm of path) until the last layers under the supported tips.

## 3. Inference

- The reference rasterises the area needing support onto its grid and keeps whole
  cells ("pillars"). This makes support easy to break away and lets the user edit it
  cell by cell; its manual support editing works on the same grid.
- Its dense support layers (0 in this job) and its separation layers behave as their
  names say. That was not separately measured.

## 4. SliceCraft's design

Two styles share one pipeline. **Pillars** is the default: easy removal and less
material, the reference's strengths. **Areas** holds every point of a surface.

The pipeline:

- **Overhang:** layer *i* overhangs where it reaches past layer *i − 1* by more than
  `h · tan(angle)`.
- **Columns:** the overhang areas are stacked from the top down into columns, and a
  column is cut wherever the model occupies a layer. On a slope each layer only
  adds a thin strip, and the strips of neighbouring layers are separated by the
  band each layer may reach out unsupported. Each neighbouring pair is closed, so
  the strips join into one area. A slope of about 60° is supported, and a test
  covers it.
- **Bed only:** with "from the bed only", columns that do not reach the bed are
  dropped from the bottom up.
- **Clearance:** each layer keeps the horizontal gap from the model beside it and
  the bottom gap from the model below. Anything left narrower than a line is
  dropped.
- **Dense layers:** the interface layers under the model, the floor layers on top
  of the model and the base layers on the bed are dense. The body lines stack in
  one direction (or a grid), and the dense layers cross them.

- **Pillars:** each layer's column area is snapped to square cells on a grid fixed
  to the bed origin, so the pillars stack exactly. A cell becomes a pillar when the
  column covers at least 3 of 9 sample points (about a third of the cell). An
  island smaller than a cell still gets the cell it sits in. Pillars are one line
  width apart, so each breaks away on its own.
- **Zigzag:** each pillar or area is filled with one zigzag. The lines are joined
  wherever the link stays inside the shape, which is checked with
  `Region::segment_inside`.
- **Defaults:** dense interface layers are off, as in the reference, because they
  make support harder to remove. The pillar size is 2 mm.

Why regions underneath: the columns, clearances and dense zones are computed
exactly, and pillars are a final snapping step. Editable cells and blockers can
work on the same grid later.

## 5. Measurements

| | Simplify3D 4.0.1 | SliceCraft |
|---|---|---|
| layers with support (crown) | 305 | 306 (pillars), 304 (areas) |
| support filament, 1.5 mm grid | 682 mm | **676 mm** with pillars; 1432 mm with areas |
| support filament by pillar size, our grid | — | 1.5 mm: 676 · 2 mm: 782 · 3 mm: 1773 · 4 mm: 1181 |
| body line spacing at 30 % | 1.33 mm | 1.33 mm |
| retractions, whole job | 4384 | 4927 with "only when crossing outlines" (9660 before it) |
| slice time, crown, release build | — | about 1.5 s with or without support |

The non-monotonic 3 mm figure comes from how the grid falls on this model's columns.
The best size depends on the model.

## 6. Open questions

1. Retractions: answered. Retracting only when a travel crosses the model's outline
   (so not inside the part, and not between pillars) brought the crown from 9660 to
   4927 retractions, against the reference's 4384. The remaining difference is
   likely combing: routing travel around holes instead of across them.
2. Manual editing: done as boxes. "Add" supports every surface facing down inside the
   box, at any angle, even with automatic support off. "Block" cuts the columns like
   the model does. The reference instead toggles single pillars; per-pillar clicking
   could be added on the same grid.
3. Measure the reference's dense support layers on a job that uses them.
4. A real print of the crown to judge how easily each design breaks away.
