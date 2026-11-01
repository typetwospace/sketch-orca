// privacy-fixtures: C:/Users/me
import { describe, expect, test } from 'bun:test'
import { cmakePath, defaultDepsJobs, defaultJobs, depsBuildArgs, depsConfigureArgs, depsParallelEnv, formatCommand, jobsCap, orcaBuildArgs, orcaConfigureArgs } from '../scripts/lib/cmake.ts'

const GIB = 1024 ** 3
const COMMIT = '8500fcdccaa10b5099ac20d252af3a7c560046f1'

describe('CMake command lines', () => {
  test('cmakePath turns backslashes into forward slashes', () => {
    expect(cmakePath('C:\\Users\\me\\deps')).toBe('C:/Users/me/deps')
  })

  test("deps on Windows: Ninja, Release, the download folder, no GUI dependencies, no IPP-ICV, MSVC with its static runtime, GMP and MPFR from source with MinGW-w64, Strawberry ignored", () => {
    expect(
      depsConfigureArgs({ ninja: 'C:\\Strawberry\\c\\bin\\ninja.exe', sourceDir: 'W:\\src\\deps', buildDir: 'W:\\deps', downloadDir: 'W:\\downloads', ignorePrefixes: ['C:/Strawberry/c'], platform: 'win32', mingwRoot: 'W:\\mingw\\msys64' }),
    ).toEqual([
      '-S', 'W:\\src\\deps',
      '-B', 'W:\\deps',
      '-G', 'Ninja',
      '-DCMAKE_MAKE_PROGRAM=C:/Strawberry/c/bin/ninja.exe',
      '-DCMAKE_BUILD_TYPE=Release',
      '-DDEP_DOWNLOAD_DIR=W:/downloads',
      '-DDEP_WITH_GUI=OFF',
      '-DDEP_OPENCV_IPP=OFF',
      '-DCMAKE_C_COMPILER=cl',
      '-DCMAKE_CXX_COMPILER=cl',
      '-DDEP_MSVC_STATIC_RUNTIME=ON',
      '-DDEP_MINGW_ROOT=W:/mingw/msys64',
      '-DCMAKE_IGNORE_PREFIX_PATH=C:/Strawberry/c',
    ])
  })

  test("deps on Windows need the MinGW-w64 toolchain: never the prebuilt GMP and MPFR of Orca's tree", () => {
    expect(() => depsConfigureArgs({ ninja: 'n', sourceDir: 's', buildDir: 'b', downloadDir: 'd', ignorePrefixes: [], platform: 'win32' })).toThrow(/MinGW-w64/)
  })

  test('deps on Linux: the system compilers, nothing ignored', () => {
    expect(depsConfigureArgs({ ninja: '/usr/bin/ninja', sourceDir: '/w/src/deps', buildDir: '/w/deps', downloadDir: '/w/downloads', ignorePrefixes: [], platform: 'linux' })).toEqual([
      '-S', '/w/src/deps',
      '-B', '/w/deps',
      '-G', 'Ninja',
      '-DCMAKE_MAKE_PROGRAM=/usr/bin/ninja',
      '-DCMAKE_BUILD_TYPE=Release',
      '-DDEP_DOWNLOAD_DIR=/w/downloads',
      '-DDEP_WITH_GUI=OFF',
      '-DDEP_OPENCV_IPP=OFF',
    ])
  })

  test('deps take one ignored prefix at most (ExternalProject would split a list)', () => {
    expect(() => depsConfigureArgs({ ninja: 'n', sourceDir: 's', buildDir: 'b', downloadDir: 'd', ignorePrefixes: ['/a', '/b'], platform: 'linux' })).toThrow(/only one ignored prefix/)
  })

  test("Orca: GUI off, static, our front end, the locked commit and tag, MSVC's static runtime", () => {
    expect(
      orcaConfigureArgs({
        ninja: 'C:\\n\\ninja.exe',
        sourceDir: 'W:\\src',
        buildDir: 'W:\\build',
        depsPrefix: 'W:\\deps\\OrcaSlicer_dep\\usr\\local',
        cliDir: 'R:\\engine\\cli',
        commit: COMMIT,
        tag: 'v2.4.2',
        ignorePrefixes: ['C:/Strawberry/c', 'D:/other'],
        platform: 'win32',
      }),
    ).toEqual([
      '-S', 'W:\\src',
      '-B', 'W:\\build',
      '-G', 'Ninja',
      '-DCMAKE_MAKE_PROGRAM=C:/n/ninja.exe',
      '-DCMAKE_BUILD_TYPE=Release',
      '-DSLIC3R_GUI=OFF',
      '-DSLIC3R_STATIC=ON',
      '-DSLIC3R_PCH=ON',
      '-DBUILD_TESTS=OFF',
      '-DORCA_TOOLS=OFF',
      '-DCMAKE_PREFIX_PATH=W:/deps/OrcaSlicer_dep/usr/local',
      '-DSKETCH_ORCA_CLI_DIR=R:/engine/cli',
      `-DSKETCH_ORCA_COMMIT=${COMMIT}`,
      '-DSKETCH_ORCA_TAG=v2.4.2',
      '-DCMAKE_C_COMPILER=cl',
      '-DCMAKE_CXX_COMPILER=cl',
      '-DCMAKE_POLICY_DEFAULT_CMP0091=NEW',
      '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded',
      '-DCMAKE_IGNORE_PREFIX_PATH=C:/Strawberry/c;D:/other',
    ])
  })

  test('Orca on Linux: no MSVC settings', () => {
    const args = orcaConfigureArgs({ ninja: '/n', sourceDir: '/s', buildDir: '/b', depsPrefix: '/d', cliDir: '/c', commit: COMMIT, tag: 'v2.4.2', ignorePrefixes: [], platform: 'linux' })
    expect(args.filter((a) => /MSVC|CMP0091|COMPILER=cl/.test(a))).toEqual([])
  })

  test('build steps: the deps target and the sketch-orca target, with -j', () => {
    expect(depsBuildArgs('W:\\deps', 3)).toEqual(['--build', 'W:\\deps', '--target', 'deps', '--', '-j3'])
    expect(orcaBuildArgs('/w/build', 12)).toEqual(['--build', '/w/build', '--target', 'sketch-orca', '--', '-j12'])
  })

  test('defaultJobs: SKETCH_ORCA_JOBS, else one per core but one per 2.5 GB of memory at most', () => {
    expect(defaultJobs({ SKETCH_ORCA_JOBS: '5' }, 32, 64 * GIB)).toBe(5)
    expect(defaultJobs({ SKETCH_ORCA_JOBS: 'x' }, 32, 64 * GIB)).toBe(25)
    expect(defaultJobs({}, 8, 64 * GIB)).toBe(8)
    expect(defaultJobs({}, 8, 2 * GIB)).toBe(1)
    expect(defaultDepsJobs({})).toBe(3)
    expect(defaultDepsJobs({ SKETCH_ORCA_DEPS_JOBS: '1' })).toBe(1)
  })

  test('SKETCH_ORCA_JOBS caps the dependency build too: one dependency at a time, and <jobs> compilers in it', () => {
    expect(jobsCap({})).toBeUndefined()
    expect(jobsCap({ SKETCH_ORCA_JOBS: 'x' })).toBeUndefined()
    expect(jobsCap({ SKETCH_ORCA_JOBS: '0' })).toBeUndefined()
    expect(jobsCap({ SKETCH_ORCA_JOBS: '8' })).toBe(8)
    expect(defaultDepsJobs({ SKETCH_ORCA_JOBS: '8' })).toBe(1)
    expect(defaultDepsJobs({ SKETCH_ORCA_JOBS: '8', SKETCH_ORCA_DEPS_JOBS: '2' })).toBe(2)
    expect(depsParallelEnv('win32', {})).toEqual({})
    expect(depsParallelEnv('linux', {})).toEqual({})
    expect(depsParallelEnv('win32', { SKETCH_ORCA_JOBS: '8' })).toEqual({ CMAKE_BUILD_PARALLEL_LEVEL: '1', CL_MPCount: '8', _CL_: '/MP8', DEP_MINGW_JOBS: '8' })
    expect(depsParallelEnv('linux', { SKETCH_ORCA_JOBS: '8' })).toEqual({ CMAKE_BUILD_PARALLEL_LEVEL: '8' })
  })

  test('formatCommand quotes arguments with spaces or quotes', () => {
    expect(formatCommand('C:\\Program Files\\CMake\\bin\\cmake.exe', ['-S', 'a b', 'x"y', 'plain'])).toBe('"C:\\Program Files\\CMake\\bin\\cmake.exe" -S "a b" "x\\"y" plain')
  })
})
