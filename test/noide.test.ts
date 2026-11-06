import { afterEach, describe, expect, test } from 'bun:test'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { assertNoIdeProjects, ideProjectTraces, isIdeProjectTrace } from '../scripts/lib/noide.ts'
import { StepError } from '../scripts/lib/run.ts'

const dirs: string[] = []
function tmp(): string {
  const d = mkdtempSync(join(tmpdir(), 'noide-'))
  dirs.push(d)
  return d
}
afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true })
})

function put(root: string, rel: string, text = 'x'): string {
  const p = join(root, ...rel.split('/'))
  mkdirSync(join(p, '..'), { recursive: true })
  writeFileSync(p, text)
  return p
}

describe('no IDE in a build', () => {
  test('a CompilerIdFortran folder or a .vfproj file, in any letter case, is a trace of a Fortran check', () => {
    expect(isIdeProjectTrace('CompilerIdFortran', true)).toBe(true)
    expect(isIdeProjectTrace('compileridfortran', true)).toBe(true)
    expect(isIdeProjectTrace('CompilerIdFortran.vfproj', false)).toBe(true)
    expect(isIdeProjectTrace('X.VFPROJ', false)).toBe(true)
    expect(isIdeProjectTrace('CompilerIdCXX', true)).toBe(false)
    expect(isIdeProjectTrace('CompilerIdC.vcxproj', false)).toBe(false)
    // A file named like the folder is not the folder.
    expect(isIdeProjectTrace('CompilerIdFortran', false)).toBe(false)
  })

  test('finds what Eigen\'s check left in a dependency build, and nothing in a clean one', () => {
    const root = tmp()
    put(root, 'dep_Eigen-prefix/src/dep_Eigen-build/CMakeFiles/3.29.2/CompilerIdCXX/CMakeCXXCompilerId.cpp')
    put(root, 'dep_TBB-prefix/src/dep_TBB-build/tbb.vcxproj')
    expect(ideProjectTraces(root)).toEqual([])
    const folder = join(root, 'dep_Eigen-prefix', 'src', 'dep_Eigen-build', 'test', 'CMakeFiles', 'CheckFortran', 'CMakeFiles', '3.29.2', 'CompilerIdFortran')
    put(root, 'dep_Eigen-prefix/src/dep_Eigen-build/test/CMakeFiles/CheckFortran/CMakeFiles/3.29.2/CompilerIdFortran/CompilerIdFortran.vfproj')
    const loose = put(root, 'other/elsewhere.vfproj')
    // The folder is named once; what is in it is not looked into.
    expect(ideProjectTraces(root).sort()).toEqual([folder, loose].sort())
  })

  test('a folder that does not exist has no traces', () => {
    expect(ideProjectTraces(join(tmp(), 'missing'))).toEqual([])
  })

  test('assertNoIdeProjects fails the build naming what it found, and passes a clean folder', () => {
    const root = tmp()
    put(root, 'a/b.vcxproj')
    expect(() => assertNoIdeProjects([root, join(root, 'missing')], 'deps')).not.toThrow()
    const p = put(root, 'a/CMakeFiles/CompilerIdFortran.vfproj')
    let err: unknown
    try {
      assertNoIdeProjects([root], 'deps')
    } catch (e) {
      err = e
    }
    expect(err).toBeInstanceOf(StepError)
    expect((err as Error).message).toContain('deps: a configure step made a Fortran project for Visual Studio')
    expect((err as Error).message).toContain(p)
    expect((err as Error).message).toContain('devenv.exe')
  })

  test('build runs the check after the dependency configure and build and after the engine build, and MSBuild leaves no nodes behind', () => {
    const orca = readFileSync(resolve(import.meta.dir, '..', 'scripts', 'orca.ts'), 'utf8')
    expect(orca.match(/assertNoIdeProjects\(\[w\.deps\], 'deps'\)/g)).toHaveLength(2)
    expect(orca).toContain("assertNoIdeProjects([w.build], 'orca')")
    // The record of a finished dependency build is written only after the check.
    expect(orca.lastIndexOf("assertNoIdeProjects([w.deps], 'deps')")).toBeLessThan(orca.indexOf("writeStamp(w.state, 'deps', inputs, record)"))
    expect(orca).toContain("MSBUILDDISABLENODEREUSE: '1'")
  })
})
