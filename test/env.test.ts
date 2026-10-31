import { describe, expect, test } from 'bun:test'
import { join } from 'node:path'
import {
  cleanWinBuildPath,
  cmakeCandidates,
  cmakeIgnorePrefixes,
  findOnPath,
  finishBuildEnv,
  firstExisting,
  gitCandidates,
  isBannedWinDir,
  isLeakyEnv,
  LEAKY_ENV,
  ninjaCandidates,
  normWinDir,
  parseSetOutput,
  perlCandidates,
  sourcePathTrimEnv,
  splitPathList,
  unixBuildPath,
  unixBuildPrograms,
  vcvarsBaseEnv,
  winBaselinePath,
} from '../scripts/lib/env.ts'

const VC = 'C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Tools\\MSVC\\14.42.34433\\bin\\HostX64\\x64'

describe('PATH cleaning (Windows)', () => {
  test('normWinDir: lower case, backslashes, no trailing separator, drive roots kept', () => {
    expect(normWinDir(' C:/Strawberry/Perl/Bin/ ')).toBe('c:\\strawberry\\perl\\bin')
    expect(normWinDir('C:\\')).toBe('c:\\')
    expect(normWinDir('D:\\Tools\\\\')).toBe('d:\\tools')
  })

  test('splitPathList splits on the platform separator and drops empty entries', () => {
    expect(splitPathList('C:\\a; ;C:\\b;;', 'win32')).toEqual(['C:\\a', 'C:\\b'])
    expect(splitPathList('/usr/bin::/bin', 'linux')).toEqual(['/usr/bin', '/bin'])
    expect(splitPathList(undefined, 'darwin')).toEqual([])
  })

  test('banned folders: Strawberry MinGW, Git MSYS tools, MSYS2 and Cygwin', () => {
    for (const d of ['C:\\Strawberry\\c\\bin', 'c:/strawberry/c', 'C:\\Strawberry\\c\\x86_64-w64-mingw32\\bin', 'C:\\Program Files\\Git\\usr\\bin', 'C:\\Program Files\\Git\\mingw64\\bin', 'C:\\Program Files\\Git\\bin', 'C:\\msys64\\usr\\bin', 'C:\\cygwin64\\bin']) {
      expect(isBannedWinDir(d)).toBe(true)
    }
    for (const d of ['C:\\Strawberry\\perl\\bin', 'C:\\Strawberry\\perl\\site\\bin', 'C:\\Program Files\\Git\\cmd', 'C:\\Windows\\System32', VC, 'C:\\Users\\me\\cygwinx']) {
      expect(isBannedWinDir(d)).toBe(false)
    }
  })

  test('cleanWinBuildPath keeps vcvars order, drops banned folders and duplicates, then adds the tool folders', () => {
    const vcvars = [VC, 'C:\\Strawberry\\c\\bin', 'C:\\WINDOWS\\system32', 'C:\\Windows\\System32\\', 'C:\\Program Files\\Git\\usr\\bin', 'C:\\Windows', '', 'C:\\']
    const out = cleanWinBuildPath(vcvars, ['C:\\Strawberry\\perl\\bin\\', 'C:\\Program Files\\Git\\cmd', 'c:\\windows'])
    expect(out).toEqual([VC, 'C:\\WINDOWS\\system32', 'C:\\Windows', 'C:\\', 'C:\\Strawberry\\perl\\bin', 'C:\\Program Files\\Git\\cmd'])
  })

  test('cleanWinBuildPath refuses to add a banned tool folder', () => {
    expect(() => cleanWinBuildPath([VC], ['C:\\Strawberry\\c\\bin'])).toThrow(/refusing/)
  })

  test('winBaselinePath is the system folders only', () => {
    expect(winBaselinePath('C:\\Windows')).toEqual([join('C:\\Windows', 'System32'), 'C:\\Windows', join('C:\\Windows', 'System32', 'Wbem'), join('C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0')])
  })

  test('parseSetOutput reads NAME=value lines and skips the =C: entries', () => {
    expect(parseSetOutput('=C:=C:\\work\r\nPath=C:\\a;C:\\b\r\nINCLUDE=x=y\r\n\r\nbroken\r\n')).toEqual({ Path: 'C:\\a;C:\\b', INCLUDE: 'x=y' })
  })

  test('finishBuildEnv drops variables that steer the build (any case on Windows) and sets PATH', () => {
    const env = finishBuildEnv({ Path: 'C:\\old', cc: 'gcc', CXX: 'g++', Cmake_Prefix_Path: 'C:\\x', PERL5LIB: 'y', KEEP: '1' }, ['C:\\a', 'C:\\b'], 'win32')
    expect(env).toEqual({ KEEP: '1', PATH: 'C:\\a;C:\\b', CMAKE_POLICY_VERSION_MINIMUM: '3.5' })
    const unix = finishBuildEnv({ PATH: '/x', CC: 'clang', cc: 'kept: case matters here', HOME: '/home/me' }, ['/usr/bin', '/bin'], 'linux')
    expect(unix).toEqual({ cc: 'kept: case matters here', HOME: '/home/me', PATH: '/usr/bin:/bin', CMAKE_POLICY_VERSION_MINIMUM: '3.5' })
    expect(LEAKY_ENV).toContain('CMAKE_TOOLCHAIN_FILE')
  })

  test('the MSVC and CMake steering variables and every <Package>_ROOT are leaky', () => {
    const polluted = {
      CL: '/DPOLLUTED', _CL_: '/DX', LINK: '/NODEFAULTLIB', _LINK_: 'x.lib', CMAKE_LIBRARY_PATH: 'C:/Strawberry/c/lib', CMAKE_INCLUDE_PATH: 'C:/x', CMAKE_PROGRAM_PATH: 'C:/y',
      CMAKE_FRAMEWORK_PATH: 'C:/z', DESTDIR: 'D:/stage', ZLIB_ROOT: 'C:/zlib', OpenSSL_ROOT: 'C:/ssl', Boost_Root: 'C:/b', KEEP: '1', SystemRoot: 'C:\\Windows',
    }
    expect(finishBuildEnv(polluted, ['C:\\a'], 'win32')).toEqual({ KEEP: '1', SystemRoot: 'C:\\Windows', PATH: 'C:\\a', CMAKE_POLICY_VERSION_MINIMUM: '3.5' })
    expect(isLeakyEnv('openssl_root', 'win32')).toBe(true)
    expect(isLeakyEnv('_ROOT', 'win32')).toBe(false)
    // Case matters on Linux and macOS: the variables are upper case there.
    expect(isLeakyEnv('ZLIB_ROOT', 'linux')).toBe(true)
    expect(isLeakyEnv('zlib_root', 'linux')).toBe(false)
    expect(isLeakyEnv('cl', 'linux')).toBe(false)
  })

  test('vcvarsBaseEnv: the caller environment without PATH, INCLUDE, LIB, LIBPATH, EXTERNAL_INCLUDE and the leaky variables', () => {
    const base = vcvarsBaseEnv({ PATH: 'C:\\old', Include: 'C:\\polluted\\include', LIB: 'C:\\polluted\\lib', LIBPATH: 'x', EXTERNAL_INCLUDE: 'y', CL: '/DX', ZLIB_ROOT: 'z', USERNAME: 'me', TEMP: 'C:\\t', NOTSET: undefined }, ['C:\\Windows\\System32', 'C:\\Windows'])
    expect(base).toEqual({ USERNAME: 'me', TEMP: 'C:\\t', Path: 'C:\\Windows\\System32;C:\\Windows' })
  })

  test('sourcePathTrimEnv: on Windows, CL trims each folder from __FILE__, longest first, quoted with a doubled backslash', () => {
    expect(sourcePathTrimEnv('win32', ['C:\\Users\\me\\AppData\\Local\\SketchDev\\orca-engine', 'C:/Users/me/repo/engine/', 'C:\\Users\\me\\AppData\\Local\\SketchDev\\orca-engine\\'])).toEqual({
      CL: '/d1trimfile:"C:\\Users\\me\\AppData\\Local\\SketchDev\\orca-engine\\\\" /d1trimfile:"C:\\Users\\me\\repo\\engine\\\\"',
    })
    expect(sourcePathTrimEnv('win32', ['C:\\Users\\John Smith\\w']).CL).toBe('/d1trimfile:"C:\\Users\\John Smith\\w\\\\"')
    expect(sourcePathTrimEnv('win32', [])).toEqual({})
    // Linux and macOS (not yet tried): nothing; CFLAGS would change what GMP's configure picks.
    expect(sourcePathTrimEnv('linux', ['/home/me/w'])).toEqual({})
    expect(sourcePathTrimEnv('darwin', ['/Users/me/w'])).toEqual({})
  })
})

describe('tools', () => {
  test('firstExisting takes the first candidate that exists', () => {
    const have = new Set(['/b', '/c'])
    expect(firstExisting([undefined, '/a', '/b', '/c'], (p) => have.has(p))).toBe('/b')
    expect(firstExisting(['/a'], () => false)).toBeUndefined()
  })

  test('cmakeIgnorePrefixes: the prefix of a CMake inside a toolchain folder, with forward slashes', () => {
    const root = process.platform === 'win32' ? 'C:\\Strawberry\\c' : '/opt/strawberry/c'
    const cmake = join(root, 'bin', 'cmake.exe')
    expect(cmakeIgnorePrefixes(cmake, (p) => p === join(root, 'include'))).toEqual([root.replace(/\\/g, '/')])
    expect(cmakeIgnorePrefixes(cmake, (p) => p === join(root, 'lib'))).toEqual([root.replace(/\\/g, '/')])
    expect(cmakeIgnorePrefixes(cmake, () => false)).toEqual([])
  })

  test('candidates: the override first, then Visual Studio, Program Files and Strawberry on Windows', () => {
    const vs = 'C:\\VS'
    expect(cmakeCandidates('win32', vs, { SKETCH_ORCA_CMAKE: 'D:\\cmake.exe', ProgramFiles: 'C:\\PF' })).toEqual([
      'D:\\cmake.exe',
      join(vs, 'Common7', 'IDE', 'CommonExtensions', 'Microsoft', 'CMake', 'CMake', 'bin', 'cmake.exe'),
      join('C:\\PF', 'CMake', 'bin', 'cmake.exe'),
      'C:\\Strawberry\\c\\bin\\cmake.exe',
    ])
    expect(ninjaCandidates('win32', undefined, { ProgramFiles: 'C:\\PF' })).toEqual([join('C:\\PF', 'CMake', 'bin', 'ninja.exe'), 'C:\\Strawberry\\c\\bin\\ninja.exe'])
    expect(perlCandidates('win32', {})).toEqual(['C:\\Strawberry\\perl\\bin\\perl.exe'])
    expect(gitCandidates('win32', { ProgramFiles: 'C:\\PF' })).toEqual([join('C:\\PF', 'Git', 'cmd', 'git.exe')])
  })

  test('candidates on Linux and macOS are the usual system and Homebrew places', () => {
    expect(cmakeCandidates('linux', undefined, {})).toEqual(['/usr/local/bin/cmake', '/usr/bin/cmake', '/opt/homebrew/bin/cmake'])
    expect(ninjaCandidates('darwin', undefined, { SKETCH_ORCA_NINJA: '/x/ninja' })[0]).toBe('/x/ninja')
    expect(perlCandidates('linux', {})).toEqual(['/usr/bin/perl', '/usr/local/bin/perl'])
    expect(gitCandidates('darwin', {})).toEqual(['/usr/bin/git', '/usr/local/bin/git', '/opt/homebrew/bin/git'])
  })

  test('unixBuildPath: tool folders first, then the system folders (Homebrew on macOS), no duplicates', () => {
    expect(unixBuildPath('linux', ['/opt/cmake/bin/', '/usr/bin'])).toEqual(['/opt/cmake/bin', '/usr/bin', '/usr/local/bin', '/bin', '/usr/sbin', '/sbin'])
    expect(unixBuildPath('darwin', ['/opt/homebrew/bin'])).toEqual(['/opt/homebrew/bin', '/usr/local/bin', '/usr/bin', '/bin', '/usr/sbin', '/sbin'])
  })

  test('unixBuildPrograms: compilers, make and the autotools GMP and MPFR need', () => {
    expect(unixBuildPrograms('linux')).toEqual([['cc'], ['c++'], ['make'], ['m4'], ['autoreconf'], ['automake'], ['libtoolize']])
    expect(unixBuildPrograms('darwin').at(-1)).toEqual(['glibtoolize', 'libtoolize'])
  })

  test('findOnPath: the first folder, then the first accepted name', () => {
    const have = new Set(['/usr/bin/libtoolize', '/opt/homebrew/bin/glibtoolize', '/usr/bin/cc'])
    const exists = (p: string) => have.has(p)
    expect(findOnPath(['glibtoolize', 'libtoolize'], ['/opt/homebrew/bin', '/usr/bin'], exists)).toBe('/opt/homebrew/bin/glibtoolize')
    expect(findOnPath(['glibtoolize', 'libtoolize'], ['/usr/bin'], exists)).toBe('/usr/bin/libtoolize')
    expect(findOnPath(['m4'], ['/usr/bin'], exists)).toBeUndefined()
  })
})
