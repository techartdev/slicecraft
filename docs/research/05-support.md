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

## 4. SliceCraft's design (and why it differs)

Region-based rather than pillars:

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

Why regions:
- They follow the model exactly, so surfaces are held everywhere, not just at
  grid points.
- They need no grid-size setting to get right.
- They share all their geometry with the rest of the pipeline.

The cost is more material. On the crown we print about 39 m of support path against
the reference's 20.5 m, on 304 layers against its 305. Pillars, editable cells and
blockers can be added on top of the region design later; `support_pillar_size` is
kept in the schema for that.

## 5. Measurements

| | Simplify3D 4.0.1 | SliceCraft |
|---|---|---|
| layers with support (crown) | 305 | 304 |
| support path | 20.5 m | 39.1 m (+ 2.2 m dense base) |
| body line spacing at 30 % | 1.33 mm | 1.33 mm |
| slice time, crown, release build | — | 2.1 s with or without support (support stages run in parallel) |

## 6. Open questions

1. Connect neighbouring support lines into zigzags, as the reference does. That
   would cut retractions; on the crown we make about 9,600.
2. Pillar or grid quantisation as an option, and manual support editing and
   blockers. Those need a support preview before slicing.
3. Measure the reference's dense support layers on a job that uses them.
4. A real print of the crown to judge how easily each design breaks away.
