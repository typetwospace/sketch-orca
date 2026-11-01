# Contributing to sketch-orca

sketch-orca does not accept outside contributions: pull requests, patches and issues are closed.
It is free software all the same. Under the GNU Affero General Public License, version 3
(`LICENSE`), you may study it, change it, build it and share your own modified version, for
example in a fork of your own, as long as you keep to that licence (and to the MIT licence for
`protocol/`).

## Changes made in this repository

The maintainers follow these rules for their own changes:

- Run `bun run test` and `bun run typecheck`. A change to `cli/` or `patches/` also needs a build
  and a passing `bun run engine smoke`.
- Keep each file's line endings (`.gitattributes` holds `* -text`; the scripts, `cli/` and the
  patches use LF).
- Date every change to OrcaSlicer's files, as section 5(a) of the licence asks: a patch's header
  (the lines before its diff) has a title line, `sketch-orca patch NNNN: ...`, and then the line
  "Modified for Sketch, <date>." (the form `stage` reads into `LICENSES/README.txt`), as the
  patches here do, and the lines it adds carry a dated comment. A file that takes code from
  OrcaSlicer says so under "Carries OrcaSlicer code", naming the OrcaSlicer file and the date.
- A change to the commands or files the engine reads and writes changes `protocol/`: the
  specification, the schemas and their examples, which `test/protocol.test.ts` checks against the
  engine's validators.
- A change is licensed under the licence of the files it changes or adds: `protocol/` under the
  MIT licence (`protocol/LICENSE`), everything else under the GNU Affero General Public License,
  version 3 (`LICENSE`), the licence of OrcaSlicer, which sketch-orca is a modified version of.
