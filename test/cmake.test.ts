// privacy-fixtures: C:/Users/me
import { describe, expect, test } from 'bun:test'
import { cmakePath, defaultDepsJobs, defaultJobs, depsBuildArgs, depsConfigureArgs, depsParallelEnv, formatCommand, jobsCap, orcaBuildArgs, orcaConfigureArgs, WIN32_DEP_CMAKE_OPTS } from '../scripts/lib/cmake.ts'

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
      '-DDEP_CMAKE_OPTS:STRING=-DCMAKE_Fortran_COMPILER:STRING=NOTFOUND;-DBUILD_TESTING:BOOL=OFF;-DEIGEN_BUILD_TESTING:BOOL=OFF',
      '-DDEP_MINGW_ROOT=W:/mingw/msys64',
      '-DCMAKE_IGNORE_PREFIX_PATH=C:/Strawberry/c',
    ])
  })

  test('deps on Windows: no configure step can run devenv.exe (no Fortran check, Eigen without its tests)', () => {
    const args = depsConfigureArgs({ ninja: 'n', sourceDir: 's', buildDir: 'b', downloadDir: 'd', ignorePrefixes: [], platform: 'win32', mingwRoot: 'm' })
    const opts = args.filter((a) => a.startsWith('-DDEP_CMAKE_OPTS'))
    expect(opts).toHaveLength(1)
    // Typed STRING, so CMake keeps the list as it is; each item one -D for every dependency.
    expect(opts[0]!.slice('-DDEP_CMAKE_OPTS:STRING='.length).split(';')).toEqual(WIN32_DEP_CMAKE_OPTS)
    // check_language skips a language whose compiler variable is defined; NOTFOUND reads false.
    expect(WIN32_DEP_CMAKE_OPTS).toContain('-DCMAKE_Fortran_COMPILER:STRING=NOTFOUND')
    expect(WIN32_DEP_CMAKE_OPTS).toContain('-DBUILD_TESTING:BOOL=OFF')
    expect(WIN32_DEP_CMAKE_OPTS).toContain('-DEIGEN_BUILD_TESTING:BOOL=OFF')
    // No item holds a space or a ';' of its own, and none is a FILEPATH (made absolute, so true).
    for (const o of WIN32_DEP_CMAKE_OPTS) expect(o).toMatch(/^-D[A-Za-z_]+:(STRING|BOOL)=[^\s;]+$/)
    // Linux and macOS set DEP_CMAKE_OPTS in their own deps-*.cmake, and run no IDE.
    expect(depsConfigureArgs({ ninja: 'n', sourceDir: 's', buildDir: 'b', downloadDir: 'd', ignorePrefixes: [], platform: 'linux' }).some((a) => a.includes('DEP_CMAKE_OPTS'))).toBe(false)
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
    // A CMake list's ';' would end the command in a POSIX shell.
    expect(formatCommand('cmake', ['-DL:STRING=a;b'])).toBe('cmake "-DL:STRING=a;b"')
  })
})
