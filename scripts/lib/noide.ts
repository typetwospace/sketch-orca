// No build step may start Visual Studio's IDE (devenv.exe) or any other program with a window
// (sketch-orca, 2026-11-06). With a Visual Studio generator, which Orca's deps use for each CMake
// dependency, CMake identifies a Fortran compiler by writing a Fortran project
// (CMakeFiles/<version>/CompilerIdFortran/CompilerIdFortran.vfproj) and running devenv.exe on it:
// check_language(Fortran), enable_language(Fortran) and project(... Fortran) all do. The dependency
// build is configured so that none of them runs (cmake.ts WIN32_DEP_CMAKE_OPTS); this check holds
// it to that. After each build, `build` looks through the build folder, and a CompilerIdFortran
// folder or a .vfproj file anywhere in it fails the build, naming them.

import { readdirSync } from 'node:fs'
import { join } from 'node:path'
import { StepError } from './run.ts'

/** What a Fortran compiler check of a Visual Studio generator leaves: a CompilerIdFortran folder or a .vfproj file. */
export function isIdeProjectTrace(name: string, isDir: boolean): boolean {
  return isDir ? name.toLowerCase() === 'compileridfortran' : name.toLowerCase().endsWith('.vfproj')
}

/**
 * Every CompilerIdFortran folder and .vfproj file under `root` (its contents not looked into), in
 * the order found; empty when there is none or `root` does not exist. Links and junctions are not
 * followed.
 */
export function ideProjectTraces(root: string): string[] {
  const found: string[] = []
  const stack = [root]
  while (stack.length > 0) {
    const dir = stack.pop()!
    let entries
    try {
      entries = readdirSync(dir, { withFileTypes: true })
    } catch (err) {
      const code = (err as NodeJS.ErrnoException).code
      if (code === 'ENOENT' || code === 'ENOTDIR') continue
      throw err
    }
    for (const e of entries) {
      const p = join(dir, e.name)
      if (e.isSymbolicLink()) continue
      if (isIdeProjectTrace(e.name, e.isDirectory())) found.push(p)
      else if (e.isDirectory()) stack.push(p)
    }
  }
  return found
}

/** Fails the build (StepError) when any of `roots` holds a Fortran project of a Visual Studio generator. */
export function assertNoIdeProjects(roots: string[], what: string): void {
  const found = roots.flatMap((r) => ideProjectTraces(r))
  if (found.length === 0) return
  const shown = found.slice(0, 10).join(', ')
  throw new StepError(
    `${what}: a configure step made a Fortran project for Visual Studio, which runs devenv.exe (the IDE) on it: ${shown}${found.length > 10 ? ` and ${found.length - 10} more` : ''}. No build may start the IDE; find the dependency that asks for Fortran and stop it (scripts/lib/cmake.ts WIN32_DEP_CMAKE_OPTS, BUILDING.md), then run build --fresh`,
  )
}
