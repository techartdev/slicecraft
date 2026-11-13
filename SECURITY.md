# Security

## Reporting

Open a private security advisory on the repository. Do not file a public issue for a
vulnerability in a parser or in emitted G-code.

## What counts as a vulnerability here

- A malformed STL/OBJ/3MF/project file causing a panic, an unbounded allocation, an
  integer overflow or a hang.
- Emitted G-code that is unsafe to run on a printer (a command sequence that drives
  an axis past its limit or heats without a target) for a valid document and a valid
  profile.
- A path or URL read from a document that reaches a file outside the document's own
  folder.
- Any `unsafe`, unwrap or panic reachable in non-test code.

## What does not

- A wrong slicing result on a model that does not satisfy the documented
  requirements — file it as a bug.
- Anything requiring a malicious local profile that the user authored themselves,
  unless it defeats validation.

## Design promises

No panics and no `unsafe` in non-test code; every input-derived size is capped
(`MAX_TRIANGLES`, `MAX_FACETS_DECLARED`, `MAX_LAYERS`); every parser returns a typed
error; the agent surface can only reach engine commands.
