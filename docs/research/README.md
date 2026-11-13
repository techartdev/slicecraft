# Research notes

Numbered, one question each. Every note has the same shape: question, method,
evidence (observations and measurements), inference, what it changes in the design,
open questions. Method rules are in [../PLAN.md](../PLAN.md) §5.

| # | Note | Question | Status |
|---|---|---|---|
| R2 | [02-feature-inventory.md](02-feature-inventory.md) | What can a person do in the reference, and where does SliceCraft stand on each item? | done |
| R3 | — | Seam placement and path ordering: what rules are observable in emitted G-code? | open |
| R4 | — | Infill pattern geometry: spacing, anchoring, clipping | open |
| R5 | [05-support.md](05-support.md) | How does the reference build support, and what does SliceCraft build? | done (first pass) |
| R6 | [06-extrusion-model.md](06-extrusion-model.md) | Extrusion model: how E follows width, height and speed | done |
| R7 | — | Dialect differences across flavours | open |
| R9 | [09-fff-profiles.md](09-fff-profiles.md) | How is a Simplify3D profile stored, and what do its values mean in SliceCraft's terms? | done |
| R8 | — | Time and material estimates | open |

## Evidence base

The running application, observed black-box: the G-code it writes for our own
models and profiles, its profile files, and screenshots of its windows. Public
documentation and published specifications (RepRap, Marlin, Klipper, STL, 3MF,
binary G-code).

## Clean-room statement

Notes describe behaviour and measurements. They do not quote the reference's
user-visible text, and no third-party code, profile file, icon, model or
documentation text is reproduced here.
