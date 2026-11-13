# R9 — Simplify3D `.fff` profiles: format and meaning

> **Research note 9.** Question: *how is a Simplify3D profile stored, and what do
> its values mean in SliceCraft's terms, so that an imported profile prints the
> same way?* Method: reading `.fff` files exported by the operator's own copies of
> Simplify3D 4.0 and 5.1.2, and comparing the G-code those versions wrote against
> the settings summary they put in each file's header. Result: the importer in
> `crates/io/src/fff.rs`.
>
> **Clean room.** The element names of the file appear in the importer because it
> has to recognise them (see AGENTS.md, "Clean room"). No profile file, label, help
> text or default value of the reference is in this repository. The operator's files
> stay outside it; the tests use profiles written by hand.

## 1. Evidence base

| Source | Count | What it gave |
|---|---|---|
| `.fff` files exported by the operator (4.0.0 and 5.1.2) | 16 | both file generations, element names, value forms |
| G-code written by 4.0.x and 5.1.2 for the operator's jobs | 158 with a version header | the settings summary in the header, and the speeds, widths, temperatures and order actually used |
| One `.fff` that its contents show was converted by another tool | 1 | what hostile-but-plausible input looks like |

## 2. The file (evidence)

- XML 1.0, one `<profile name=… version=… app=…>` root. 5.x adds `technology="fff"`.
- One element per setting, at the top level, plus:
  - `<extruder name=…>` blocks (one per extruder);
  - `<temperatureController name=…>` blocks (extruder, bed and, in 5.x, chamber);
  - a fan schedule.
- **Two generations.**

  | | 4.x | 5.x |
  |---|---|---|
  | choices | numbers or names as text | `<option value=… displayValue=… selected="1"/>` lists |
  | schedules | `<setpoint layer=… temperature=…/>`, `<setpoint layer=… speed=…/>` | `<option value="layer\|value"/>` |
  | script lines joined by | `,` | `\|` |
  | number format | as typed (`0.3`) | five decimals (`0.30000`) |

  Many settings were renamed between the generations (for example the perimeter
  count, the retraction distance and the build volume). The importer accepts both
  names for each.
- 4.x files may carry material and quality presets (`autoConfigureMaterial`,
  `autoConfigureQuality`) next to the main settings.
- Units: speeds are mm/min. In 4.x, the feature speeds are fractions of the default
  speed ("underspeeds", e.g. `0.8`). In 5.x they are percentages.

## 3. What the values mean (measured)

Each line was measured on G-code written by the same version, from the settings
summary in that file's header.

| Behaviour | Measurement | Importer rule |
|---|---|---|
| Automatic line width | nozzle 0.4 → `W0.480` in the tool header, in 4.0 and 5.1.2. 5.1.2 does this even when the file stores `0.40000` as the width. | fixed width = 1.2 × nozzle |
| First layer width | 150 % → `W0.720` | percentage of the line width |
| Outer wall speed (4.x) | default 3600, outline underspeed 0.8 → outer walls at `F2880` | outer = default × underspeed |
| Inner wall speed (4.x) | same file → inner walls at `F3240` | inner = default × (1 + underspeed) / 2 |
| Solid infill speed (4.x) | solid underspeed 0.8 → `F2880` | percentage = underspeed × 100 |
| Heat-up order | bed set and wait, nozzle set and wait, then the user's starting script | SliceCraft heats before a custom start script, unless the script heats itself (writer change) |
| Temperatures | `temperatureSetpointLayers 1,1` with `205,60` → `M104 S205`, `M140 S60` | the first setpoint is the first-layer temperature; the first one at layer ≥ 2 is the later temperature |

## 4. Inference (not measured)

- **4.x start points.** `startPointOption` 1/2/3 is taken to mean random, nearest
  and aligned. That is the order of the three 5.x options. Every file with option 3
  also has an aligned origin set, which fits.
- **5.x infill angles.** `sparseInfillRotation` is taken as a rotation of the 4.x
  default angles 45°/−45°.
- **4.x support type.** `supportType` 0/1 is taken to mean "everywhere" and "from
  the bed only".
- **4.x thin wall types.** 0/1/2 are taken to follow the 5.x option order:
  perimeters only, single extrusions, gap fill.
- **Script variables.** `[extruder0_temperature]` and `[bed0_temperature]` in a
  starting script become the first-layer placeholders, and elsewhere the normal
  ones. `[current_layer]` and `[current_Z_position]` map directly. None of the
  operator's files uses a variable, so this is untested against real files.

## 5. Check against a real job

The operator's "Ender3 DD Solid" profile was imported, and SliceCraft sliced the STL
of a job that Simplify3D 4.0.0 had printed with the same settings:

| | Simplify3D 4.0.0 | SliceCraft |
|---|---|---|
| layers, first and top Z | 45, 5.0, 12.04 | 45, 5.0, 12.04 |
| temperatures | bed 60, nozzle 205 | bed 60, nozzle 205 |
| outer / inner wall feed | 2880 / 3240 | 2880 / 3240 |
| filament | 9578 mm | 1279 mm for one copy |

The reference job had about 7 copies on the bed: 14 to 15 outer loops per layer
against 2 for one copy. Per copy that is about 1370 mm, so ours is about 7 % less.
The number of copies is inferred from the loops; the job file does not record it.

## 6. Open questions

1. Real `.fff` files that use script variables, so the variable mapping can be
   checked.
2. A 5.x G-code made from a profile with a non-zero `sparseInfillRotation`, to
   confirm the angle inference.
3. Multi-extruder profiles. Today only the first extruder is imported, with a note.
4. Whether to also import settings from the header of a reference G-code file (it
   carries the full 4.x key set). That would recover the settings of an old job
   whose profile is lost.
