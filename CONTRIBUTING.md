# Contributing to sketch-orca

Changes are welcome as patches or pull requests. Before you send one:

- Run `bun run test` and `bun run typecheck`. A change to `cli/` or `patches/` also needs a build
  and a passing `bun run engine smoke`; say in the request whether you ran them, and on which
  platform.
- Keep each file's line endings (`.gitattributes` holds `* -text`; the scripts, `cli/` and the
  patches use LF).
- Date your changes to OrcaSlicer's files, as section 5(a) of the licence asks: a patch's header
  (the lines before its diff) has a title line, `sketch-orca patch NNNN: ...`, and then the line
  "Modified for Sketch, <date>." (the form `stage` reads into `LICENSES/README.txt`), as the
  patches here do, and the lines it adds carry a dated comment. A file that takes code from
  OrcaSlicer says so under "Carries OrcaSlicer code", naming the OrcaSlicer file and the date.
- A change to the commands or files the engine reads and writes changes `protocol/`: the
  specification, the schemas and their examples, which `test/protocol.test.ts` checks against the
  engine's validators.

## Licences of contributions

By contributing you agree that your contribution is licensed under the licence of the files it
changes or adds:

- `protocol/` under the MIT licence (`protocol/LICENSE`);
- everything else under the GNU Affero General Public License, version 3 (`LICENSE`), the licence
  of OrcaSlicer, which sketch-orca is a modified version of.

You keep the copyright in your contribution. Add your name to `AUTHORS` in the same change.

## Developer Certificate of Origin

Every commit must be signed off: it ends with a line

```
Signed-off-by: Your Name <you@example.com>
```

with the name and address you commit under (`git commit -s` adds it). The line certifies that
you wrote the change or otherwise have the right to submit it under these licences, as the
Developer Certificate of Origin says:

```
Developer Certificate of Origin
Version 1.1

Copyright (C) 2004, 2006 The Linux Foundation and its contributors.

Everyone is permitted to copy and distribute verbatim copies of this
license document, but changing it is not allowed.


Developer's Certificate of Origin 1.1

By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same open source license (unless I am
    permitted to submit under a different license), as indicated
    in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```
