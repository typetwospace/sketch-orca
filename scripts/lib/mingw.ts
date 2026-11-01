// The MinGW-w64 toolchain that builds GMP and MPFR for the Windows engine (patches/0007: Orca's
// deps/MinGW/MinGW.cmake with -DDEP_MINGW_ROOT). GMP and MPFR are built with GCC, not MSVC, so the
// pipeline unpacks an MSYS2 tree into the work folder (<work>/mingw/msys64) from pinned packages: the
// MSYS2 base system (bash and the shell tools that configure scripts use), make, m4 and diffutils
// from MSYS2's msys repository, and the GCC toolchain of its mingw64 environment (GCC, binutils, the
// mingw-w64 runtime and headers) with the packages it needs. Nothing is installed: no installer, no
// pacman, no administrator rights, nothing written outside the work folder. Each package is
// downloaded once into <work>/mingw/downloads, checked against its pinned size and SHA-256, and
// unpacked with Windows' own tar (bsdtar, which reads xz and zstd).

import { existsSync, mkdirSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { hashTree, sha256Bytes } from './files.ts'
import { readStamp, sha256File, sha256Text, writeStamp, type WorkPaths } from './paths.ts'
import { Log, runLogged, StepError } from './run.ts'

export interface MingwPackage {
  /** The package file, as MSYS2 publishes it. */
  file: string
  /** Where it is published, tried in order. */
  urls: string[]
  size: number
  /** Lower case. */
  sha256: string
}

const DISTRIB = 'https://repo.msys2.org/distrib/x86_64/'
const MSYS = 'https://repo.msys2.org/msys/x86_64/'
const MINGW64 = 'https://repo.msys2.org/mingw/mingw64/'

const pkg = (repo: string, file: string, size: number, sha256: string): MingwPackage => ({ file, urls: [`${repo}${file}`], size, sha256 })

/**
 * The MSYS2 base system of 2026-09-27, the release archive of MSYS2's installer (the same bytes on
 * repo.msys2.org and in the installer's GitHub release). Its top folder is msys64/.
 */
export const MSYS2_BASE: MingwPackage = {
  file: 'msys2-base-x86_64-20260927.tar.xz',
  urls: [`${DISTRIB}msys2-base-x86_64-20260927.tar.xz`, 'https://github.com/msys2/msys2-installer/releases/download/2026-09-27/msys2-base-x86_64-20260927.tar.xz'],
  size: 42860696,
  sha256: 'ea2f31a0b6ade63914ce441ffb022f0f6aa96982bfefa2326460a26d5fb01322',
}

/**
 * The packages unpacked over the base system (pinned 2026-11-01): make, m4 and diffutils (cmp and
 * diff, which configure and the test suites run) from the msys repository, whose own dependencies
 * the base system has; and mingw-w64-x86_64-gcc 16.2.0 with every package it depends on, the
 * closure of its dependencies in the mingw64 repository on that day. GCC's own gmp, mpfr, mpc and
 * isl packages are libraries the compiler runs on; the GMP and MPFR the engine ships are built from
 * their source packages, never taken from here.
 */
export const MINGW_PACKAGES: readonly MingwPackage[] = [
  pkg(MSYS, 'make-4.4.1-3-x86_64.pkg.tar.zst', 514683, 'af0bdba17f06fe037f0194069adaa31a8fe45f1a11381501896aea1fae37bd5d'),
  pkg(MSYS, 'm4-1.4.21-1-x86_64.pkg.tar.zst', 258635, 'b561e8871c7599502d9fae7196ee965b3fed3d7af09154eb8d2f5e4d9e5e5106'),
  pkg(MSYS, 'diffutils-3.12-1-x86_64.pkg.tar.zst', 394515, '7902c8ce3d4dd69a0f5e98dc9d5c83c17b23314ba486169db57ef6e2835ce3b6'),
  pkg(MINGW64, 'mingw-w64-x86_64-binutils-2.47-4-any.pkg.tar.zst', 6632105, 'e36c52a7fe1d104c2efd97dc74162386eabf8323f6aeb9b83aeb574b291a7da0'),
  pkg(MINGW64, 'mingw-w64-x86_64-cc-libs-16.2.0-6-any.pkg.tar.zst', 2339, '0128c6f7fc227e5862351e970e52d42370c72c896db87b8413c2699c6337a13a'),
  pkg(MINGW64, 'mingw-w64-x86_64-crt-14.0.0.r569.g34eeafb7d-1-any.pkg.tar.zst', 4000809, '0907e168f4a52c0f692e5d0a95defec69bf9fa4cb1b462c61927bfc83ab8e6ff'),
  pkg(MINGW64, 'mingw-w64-x86_64-gcc-16.2.0-6-any.pkg.tar.zst', 48527572, 'f9a78b7f257984bc8a2b62d5f93f1af45b03d4b84afaf2eafc7d90c9b8c4568e'),
  pkg(MINGW64, 'mingw-w64-x86_64-gettext-runtime-1.0-1-any.pkg.tar.zst', 427276, 'be68d7f260633284b910c588c6d82ee304a81c8817a686d2cd9df83f872c27af'),
  pkg(MINGW64, 'mingw-w64-x86_64-gmp-6.3.0-2-any.pkg.tar.zst', 591674, '8924433974c4add46cb46ea4f6ef283b5c5139d3f552375115b5580f855015cc'),
  pkg(MINGW64, 'mingw-w64-x86_64-headers-14.0.0.r569.g34eeafb7d-1-any.pkg.tar.zst', 6895520, 'eea513d19af1918b31b60e04137b1cce6e0b19145ff175db8f4ed9bd697a7457'),
  pkg(MINGW64, 'mingw-w64-x86_64-isl-0.28-1-any.pkg.tar.zst', 1533140, '03ed191718a89edc46e783deed46b8fd20ec47a09fcdefb37c6c4169b6188b08'),
  pkg(MINGW64, 'mingw-w64-x86_64-libgcc-16.2.0-6-any.pkg.tar.zst', 77153, 'e0815432c4225892b0eef57055fe507fa0b9a036436f046605f6629696bd1b88'),
  pkg(MINGW64, 'mingw-w64-x86_64-libiconv-1.19-1-any.pkg.tar.zst', 745957, '21e334d0911f25de75d3e18e0697648bcecfa9658256d600cad0827d719c2f35'),
  pkg(MINGW64, 'mingw-w64-x86_64-libstdc++-16.2.0-6-any.pkg.tar.zst', 797366, '924b47e8c3e20b6448e29d3333acc638a4f4783c1c45230e09673532f9c4f198'),
  pkg(MINGW64, 'mingw-w64-x86_64-libwinpthread-14.0.0.r569.g34eeafb7d-1-any.pkg.tar.zst', 34648, '7774d48f1b2727f54c8148e5994a4a8e464dfa2dd5a177e630ebfd522dcbe2c0'),
  pkg(MINGW64, 'mingw-w64-x86_64-mpc-1.4.1-1-any.pkg.tar.zst', 140728, 'ce024a90d59c8a591d2c88ef94a386c6367750bf9edc6a944e80815ec5d93344'),
  pkg(MINGW64, 'mingw-w64-x86_64-mpfr-4.2.2-3-any.pkg.tar.zst', 554568, '9ecbc05f1f855bc656a8f111d367f61fbd90dbbfcf469ba74d6d5dd1ec07a542'),
  pkg(MINGW64, 'mingw-w64-x86_64-tzdata-2026f-1-any.pkg.tar.zst', 225912, 'ee33831fd41045555220d250a816a2a3a57116f8b1fb7f574bf3db7079299da2'),
  pkg(MINGW64, 'mingw-w64-x86_64-windows-default-manifest-20260815-1-any.pkg.tar.zst', 2652, 'd2dd9a4f3a3362bc85ca273dff2b0f9868cfba7ba60151499b5935a049f00d74'),
  pkg(MINGW64, 'mingw-w64-x86_64-winpthreads-14.0.0.r569.g34eeafb7d-1-any.pkg.tar.zst', 48316, '4463dca6cd62bd4f7f68f21a8d447f0751147c565ce794552f83753e4f6b2289'),
  pkg(MINGW64, 'mingw-w64-x86_64-zlib-1.3.2-3-any.pkg.tar.zst', 110030, 'ffde6f4ed2d879477c93b37ff14c319baf480a2aeafc065edeceafba26b71e90'),
  pkg(MINGW64, 'mingw-w64-x86_64-zstd-1.5.7-2-any.pkg.tar.zst', 669879, '1add6705b344664f6aca108c85f79ab5bdd9e1162662bb06a4cf40a34f6e0907'),
]

/** The programs MinGW.cmake and build.sh run, in the tree. */
export const MINGW_TOOLS = ['usr/bin/bash.exe', 'usr/bin/make.exe', 'usr/bin/m4.exe', 'usr/bin/cygpath.exe', 'mingw64/bin/gcc.exe', 'mingw64/bin/strip.exe'] as const

/**
 * The licence texts of the toolchain's code that GCC links into the DLLs it makes, in the tree: the
 * mingw-w64 runtime (its DLL start-up and pseudo-relocation code; some of it under licences that ask
 * for their notices with the binaries) and libgcc (GPL 3 with the GCC Runtime Library Exception).
 * stage ships them as LICENSES/MinGW-w64/<to>.
 */
export const MINGW_RUNTIME_LICENSES: readonly { from: string; to: string }[] = [
  { from: 'mingw64/share/licenses/crt/COPYING.MinGW-w64-runtime.txt', to: 'COPYING.MinGW-w64-runtime.txt' },
  { from: 'mingw64/share/licenses/libgcc/COPYING.RUNTIME', to: 'libgcc/COPYING.RUNTIME' },
  { from: 'mingw64/share/licenses/libgcc/COPYING3', to: 'libgcc/COPYING3' },
]

/** pacman's own records in a package, which are not files of the tree. */
export const PACKAGE_RECORDS = ['.PKGINFO', '.BUILDINFO', '.MTREE', '.INSTALL'] as const

/** Where the toolchain lives: <work>/mingw/msys64, what MinGW.cmake's DEP_MINGW_ROOT names. */
export function mingwRoot(work: Pick<WorkPaths, 'mingw'>): string {
  return join(work.mingw, 'msys64')
}

/** What the toolchain is: a digest of the pinned packages, in the order they are unpacked. */
export function mingwPinsDigest(base: MingwPackage = MSYS2_BASE, packages: readonly MingwPackage[] = MINGW_PACKAGES): string {
  return sha256Text(JSON.stringify([base, ...packages].map((p) => [p.file, p.size, p.sha256])))
}

/**
 * Problems with the pins: a file named twice, a size that is not a whole number above 0, a SHA-256
 * that is not 64 lower-case hex digits, a URL that is not https or does not end in the file's name.
 */
export function mingwPinProblems(base: MingwPackage = MSYS2_BASE, packages: readonly MingwPackage[] = MINGW_PACKAGES): string[] {
  const out: string[] = []
  const seen = new Set<string>()
  for (const p of [base, ...packages]) {
    if (seen.has(p.file)) out.push(`${p.file} is pinned twice`)
    seen.add(p.file)
    if (!Number.isInteger(p.size) || p.size <= 0) out.push(`${p.file}: size ${p.size}`)
    if (!/^[0-9a-f]{64}$/.test(p.sha256)) out.push(`${p.file}: SHA-256 ${p.sha256}`)
    if (p.urls.length === 0) out.push(`${p.file}: no URL`)
    for (const u of p.urls) if (!u.startsWith('https://') || !u.endsWith(`/${p.file}`)) out.push(`${p.file}: URL ${u}`)
  }
  return out
}

/** Gets the bytes at a URL; throws when it cannot. */
export type Fetcher = (url: string) => Promise<Uint8Array>

/** A Fetcher on the global fetch, following redirects. */
export const httpFetcher: Fetcher = async (url) => {
  const res = await fetch(url, { redirect: 'follow' })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return new Uint8Array(await res.arrayBuffer())
}

/**
 * A pinned file at `path`: a copy already there is kept when its size and SHA-256 are the pin's
 * (else removed), otherwise the pin's URLs are tried in order and the first download of the pinned
 * size and SHA-256 is kept. Throws, naming what each URL gave, when none gives it.
 */
export async function ensurePinnedFile(p: MingwPackage, path: string, fetcher: Fetcher, line: (msg: string) => void): Promise<string> {
  if (existsSync(path)) {
    if (statSync(path).size === p.size && sha256File(path) === p.sha256) return path
    line(`mingw: ${path} is not the pinned ${p.file} (SHA-256 ${p.sha256}); getting it again`)
    rmSync(path, { force: true })
  }
  const failures: string[] = []
  for (const url of p.urls) {
    let bytes: Uint8Array
    try {
      line(`mingw: downloading ${url}`)
      bytes = await fetcher(url)
    } catch (err) {
      failures.push(`${url}: ${(err as Error).message}`)
      continue
    }
    const sha256 = sha256Bytes(bytes)
    if (bytes.length !== p.size || sha256 !== p.sha256) {
      failures.push(`${url}: ${bytes.length} bytes with SHA-256 ${sha256}, not ${p.size} bytes with ${p.sha256}`)
      continue
    }
    mkdirSync(dirname(path), { recursive: true })
    const partial = `${path}.partial`
    writeFileSync(partial, bytes)
    renameSync(partial, path)
    return path
  }
  throw new StepError(`cannot get ${p.file}: ${failures.join('; ')}`)
}

/** Windows' own tar.exe (bsdtar, in Windows 10 1803 and later). */
export function windowsTar(env: NodeJS.ProcessEnv = process.env): string {
  return join(env.SystemRoot ?? env.SYSTEMROOT ?? 'C:\\Windows', 'System32', 'tar.exe')
}

/** tar's arguments that unpack `archive` into `dir`, without pacman's records. */
export function unpackArgs(archive: string, dir: string): string[] {
  return ['-x', '-f', archive, '-C', dir, ...PACKAGE_RECORDS.flatMap((r) => ['--exclude', r])]
}

/**
 * The toolchain's files whose digest the stamp keeps: its programs and libraries (usr/ and
 * mingw64/), not what the MSYS2 runtime may write as it runs (tmp/, var/, dev/, etc/, home/).
 */
export function mingwTreeFile(rel: string): boolean {
  return rel === 'usr' || rel.startsWith('usr/') || rel === 'mingw64' || rel.startsWith('mingw64/')
}

/**
 * The toolchain, ready: when state/mingw.json records the pinned packages and the tree still holds
 * the files they unpacked to (their digest), it is kept; otherwise every package is fetched (or
 * taken from <work>/mingw/downloads when it is the pinned one) and the tree is unpacked again, next
 * to the old one and then swapped in. Returns the tree (mingwRoot).
 */
export async function ensureMingw(work: WorkPaths, log: Log, fetcher: Fetcher = httpFetcher, tar: string = windowsTar()): Promise<string> {
  const root = mingwRoot(work)
  const want = mingwPinsDigest()
  const stamp = readStamp(work.state, 'mingw')
  const recorded = (stamp?.detail as { treeDigest?: unknown } | undefined)?.treeDigest
  if (stamp?.inputs === want && typeof recorded === 'string' && MINGW_TOOLS.every((t) => existsSync(join(root, ...t.split('/'))))) {
    if (hashTree(root, (rel) => mingwTreeFile(rel)) === recorded) {
      log.line(`mingw: ${root} is the pinned toolchain; keeping it`)
      return root
    }
    log.line(`mingw: ${root} changed since it was unpacked; unpacking it again`)
  }
  if (!existsSync(tar)) throw new StepError(`${tar} not found: the MinGW-w64 toolchain is unpacked with Windows' own tar (Windows 10 1803 or later)`)
  rmSync(join(work.state, 'mingw.json'), { force: true })
  const downloads = join(work.mingw, 'downloads')
  const files: string[] = []
  for (const p of [MSYS2_BASE, ...MINGW_PACKAGES]) files.push(await ensurePinnedFile(p, join(downloads, p.file), fetcher, (msg) => log.line(msg)))
  const partial = join(work.mingw, 'msys64.partial')
  rmSync(partial, { recursive: true, force: true })
  mkdirSync(partial, { recursive: true })
  log.line(`mingw: unpacking ${files.length} packages into ${root}`)
  await runLogged(tar, unpackArgs(files[0]!, partial), { log })
  const tree = join(partial, 'msys64')
  if (!existsSync(join(tree, 'usr', 'bin', 'bash.exe'))) throw new StepError(`${MSYS2_BASE.file} did not unpack to msys64/ with usr/bin/bash.exe`)
  for (const f of files.slice(1)) await runLogged(tar, unpackArgs(f, tree), { log })
  const missing = MINGW_TOOLS.filter((t) => !existsSync(join(tree, ...t.split('/'))))
  if (missing.length > 0) throw new StepError(`the unpacked MinGW-w64 toolchain has no ${missing.join(', ')}`)
  const treeDigest = hashTree(tree, (rel) => mingwTreeFile(rel))
  rmSync(root, { recursive: true, force: true })
  renameSync(tree, root)
  rmSync(partial, { recursive: true, force: true })
  writeStamp(work.state, 'mingw', want, { root, treeDigest })
  log.line(`mingw: ${root} unpacked`)
  return root
}
