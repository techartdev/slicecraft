# Parity checklist

Every capability the target application has that we intend to match, one row each,
with the phase that delivers it and its presence today. Update a row only when a
test proves it, and say which test. Last updated 2026-11-13 (MVP).

Columns: **cap** = capability id (ours), **what** = behaviour, **phase**, **presence**,
**proof** = the test that proves presence.

## Import and plate

| cap | what | phase | presence | proof |
|---|---|---|---|---|
| `import.stl.ascii` | read an ASCII STL mesh | P0 | 100% | `io::tests::ascii_stl_parses` |
| `import.stl.binary` | read a binary STL mesh | P1 | 100% | `io::mesh_in::tests::binary_stl_with_solid_header_is_binary`, `truncated_binary_stl_keeps_complete_facets` |
| `import.obj` | read OBJ positions and faces | P1 | 100% | `io::mesh_in::tests::obj_quads_and_negative_indices` |
| `import.3mf` | read a 3MF package (units, transforms, several objects) | P1 | 0% | — |
| `place.translate` | move a model on the bed | P1 | 100% | `engine::tests::placement_moves_the_gcode` |
| `place.rotate` | rotate about Z | P1 | 100% | `doc::tests::rotation_and_scale_are_applied` |
| `place.scale` | uniform scale | P1 | 100% | `doc::tests::rotation_and_scale_are_applied` |
| `place.arrange` | lay several models out without overlap | P1 | 50% | `doc::tests::arrange_separates_objects` (row packing, not nesting) |
| `place.drop` | drop a model onto the bed plane | P1 | 100% | `engine::tests::import_centres_the_model_on_the_bed` (models always sit on the bed) |
| `repair.holes` | close open boundaries | P2 | 0% | — |
| `repair.normals` | reorient inverted facets | P2 | 0% | — |
| `repair.degenerate` | remove zero-area facets and duplicate vertices | P2 | 0% | — |

## Layers

| cap | what | phase | presence | proof |
|---|---|---|---|---|
| `layer.section` | intersect a mesh with a Z plane | P0 | 100% | `render::tests::slice_through_vertices_closes` |
| `layer.stitch` | join segments into closed, oriented loops | P1 | 100% | `render::tests::mid_cube_section_is_one_ccw_square`, `render::section::tests::small_gaps_are_joined_and_hopeless_chains_dropped` |
| `layer.nest` | classify outer loops vs holes | P1 | 100% | `render::tests::hollow_box_has_outer_and_hole`, `geom::region::tests::loops_with_a_hole_make_one_polygon` |
| `layer.height` | fixed layer height from the profile | P0 | 100% | `engine::pipeline::tests::schedule_has_first_layer_then_steps` |
| `layer.height.first` | distinct first layer height | P0 | 100% | `engine::pipeline::tests::schedule_has_first_layer_then_steps` |
| `layer.height.adaptive` | vary layer height by geometry | P3 | 0% | — |
| `layer.range` | per-layer-range setting overrides | P3 | 0% | — |

## Paths

| cap | what | phase | presence | proof |
|---|---|---|---|---|
| `perimeter.count` | N shells per layer | P3 | 100% | `render::paths::tests::two_perimeters_on_a_square`, `engine::tests::hole_gets_its_own_perimeter` |
| `perimeter.order` | inside-out / outside-in ordering | P3 | 50% | inside-out only; `engine::pipeline` ordering |
| `perimeter.seam` | seam placement (aligned, nearest, random) | P3 | 25% | nearest vertex only; `render::paths::tests::closed_path_starts_at_nearest_vertex` |
| `perimeter.thin` | thin-wall policy | P3 | 0% | — |
| `perimeter.gap` | gap fill between shells and infill | P3 | 0% | — |
| `infill.sparse` | sparse infill at a density | P3 | 100% | `engine::tests::cube_slices_into_real_toolpaths` |
| `infill.pattern` | rectilinear, grid, triangles, concentric, gyroid-class | P3 | 20% | rectilinear only; `render::paths::tests::diagonal_infill_stays_inside` |
| `infill.external` | separate external pattern and rotation | P3 | 0% | — |
| `infill.solid` | solid top/bottom layers with angle threshold | P3 | 75% | `engine::tests::cube_slices_into_real_toolpaths` (layer lookahead, no angle threshold) |
| `infill.dense` | dense layers near surfaces | P3 | 0% | — |
| `support.normal` | overhang support with interfaces and base layers | P4 | 0% | — |
| `support.block` | support blockers and enforcers | P4 | 0% | — |
| `adhesion.skirt` | skirt loop | P4 | 100% | `engine::tests::cube_slices_into_real_toolpaths` |
| `adhesion.brim` | brim | P4 | 0% | — |
| `adhesion.raft` | raft with separation layers | P4 | 0% | — |
| `adhesion.pillar` | prime pillar | P4 | 0% | — |
| `adhesion.shield` | ooze shield | P4 | 0% | — |
| `bridge.detect` | bridging detection feeding shells and infill | P4 | 0% | — |
| `order.travel` | path ordering with travel minimisation | P3 | 50% | `render::paths::tests::ordering_picks_nearest_and_reverses` (no combing) |

## Material and machine

| cap | what | phase | presence | proof |
|---|---|---|---|---|
| `flow.model` | extrusion from width, height and filament diameter | P3 | 100% | `io::gcode::tests::extrusion_follows_width_and_height`, `engine::tests::cube_filament_matches_hand_estimate` |
| `cool.fan` | fan schedule per layer and feature | P4 | 25% | fan from a layer; `io::gcode::tests::header_start_layers_fan_and_end` |
| `cool.slow` | slow layers for minimum layer time | P4 | 0% | — |
| `temp.schedule` | per-layer temperature and standby | P4 | 0% | — |
| `retract.basic` | retraction distance and speed | P4 | 100% | `io::gcode::tests::moving_between_paths_is_a_travel_not_an_extrusion`, `unretract_after_layer_reset_restores_the_filament` |
| `retract.wipe` | wipe and coasting | P4 | 0% | — |
| `retract.zhop` | Z-hop | P4 | 0% | — |
| `tool.multi` | per-region extruder assignment and offsets | P5 | 0% | — |
| `profile.printer` | bed, kinematics, steps/mm, flavour | P2 | 0% | — |
| `profile.material` | temperatures, flow, cooling | P2 | 0% | — |
| `profile.inherit` | profile inheritance and per-object overrides | P2 | 0% | — |

## Output

| cap | what | phase | presence | proof |
|---|---|---|---|---|
| `gcode.header` | units, absolute mode, home, temperatures | P0 | 100% | `io::gcode::tests::header_start_layers_fan_and_end` |
| `gcode.tail` | retract, lift, motors off | P0 | 100% | same test |
| `gcode.flavor` | Marlin, RepRap, Klipper command sets | P5 | 25% | flavour enum exists, differences are minimal |
| `gcode.relative-e` | relative extrusion | P5 | 0% | — |
| `gcode.arcs` | G2/G3 arcs | P5 | 0% | — |
| `gcode.thumbnail` | embedded preview thumbnail | P5 | 0% | — |
| `gcode.validate` | grammar check per flavour | P5 | 0% | — |

## Interaction and automation

| cap | what | phase | presence | proof |
|---|---|---|---|---|
| `ui.window` | a desktop window | P6 | 100% | `slicecraft` opens an eframe window (driven and screenshotted 2026-11-13) |
| `ui.viewport` | 3D bed and model view | P6 | 75% | bed, grid, volume, shaded models, orbit/pan/zoom, pick, shift-drag move; `ui_egui::camera::tests` |
| `ui.preview` | layer scrubber and toolpath view | P5 | 75% | layer slider, feature legend and toggles; `ui_egui::scene::tests` |
| `ui.settings` | settings editor | P6 | 75% | settings panel through the `profile` command |
| `cli.info` | inspect a model headlessly | P0 | 100% | `slicecraft-cli info` |
| `cli.slice` | slice headlessly | P0 | 100% | `slicecraft-cli slice` |
| `cli.json` | machine-readable output | P1 | 100% | `slicecraft-cli slice … --json` |
| `agent.control` | JSON control channel | P5 | 0% | — |
| `agent.mcp` | MCP tools bound to engine commands | P5 | 25% | `mcp::tests::import_slice_export_round_trip` (dispatch only, no transport) |
| `estimate.time` | print time estimate | P5 | 50% | constant-feed estimate, uncalibrated; `io::gcode::tests::durations_format` |
| `estimate.material` | material estimate | P5 | 75% | filament length and PLA weight; `engine::tests::cube_filament_matches_hand_estimate` |
| `printer.serial` | USB serial printer control | P6 | 0% | — |
| `printer.network` | network discovery and job upload | P6 | 0% | — |

## How this list is kept honest

- A row moves off 0% in the same change that adds its proof. A row without a proof
  is 0%, whatever the code looks like.
- The capability ids are ours; the *behaviour* column is written from observed
  behaviour and public specifications, never from another product's documentation
  text.
