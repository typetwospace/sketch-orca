// parity reference: the published engine zip downloaded with a fake GitHub (a fake release answer
// and a fake fetch), checked against SHA256SUMS and its pin, and unpacked whole or not at all.
// Hermetic: temporary folders only, no network.

import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { createHash } from 'node:crypto'
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import type { FetchLike, JsonGetter } from '../scripts/lib/github.ts'
import { DEFAULT_REFERENCE, engineRoot, fetchReference, keptReference, referenceDir, REFERENCES, SKETCH_ORCA_REPO, unpackZip, type ReferencePin } from '../scripts/lib/parity/reference.ts'
import { writeZip, type ZipInput } from './support/zip-writer.ts'

const sha256 = (b: Uint8Array | string) => createHash('sha256').update(b).digest('hex')
const REPO = 'example/tool'
const TAG = 'v0.1.0'
const DL = `https://github.com/${REPO}/releases/download/${TAG}`
const STORE = 'https://release-assets.githubusercontent.com/github-production-release-asset'

/** A staged engine's files and its manifest (stage.ts EngineManifest), as a zip's entries. */
function engineEntries(o: { stageVersion?: number; version?: string; top?: string; tamper?: boolean } = {}): ZipInput[] {
  const files: Record<string, string> = { 'bin/sketch-orca.exe': 'MZ engine', 'bin/gmp.dll': 'MZ gmp', 'resources/profiles/Vendor/machine/a.json': '{"name":"a"}', 'LICENSES/NOTICE.txt': 'notice' }
  const list = Object.keys(files)
    .sort()
    .map((p) => ({ path: p, size: Buffer.byteLength(files[p]!), sha256: sha256(files[p]!) }))
  const manifest = {
    format: 'sketch-orca-engine/1',
    stageVersion: o.stageVersion ?? 3,
    engine: { name: 'sketch-orca', version: o.version ?? '0.1.0', exe: 'bin/sketch-orca.exe', resources: 'resources', libraryPath: 'bin' },
    orca: { repo: 'OrcaSlicer/OrcaSlicer', tag: 'v2.4.2', version: '2.4.2', commit: '8'.repeat(40), publishedAt: '2026-07-07T03:33:49Z', license: 'AGPL-3.0-only' },
    platform: 'win32',
    arch: 'x64',
    patches: [],
    cli: 'c'.repeat(64),
    runtime: ['gmp.dll'],
    system: ['KERNEL32.dll'],
    smoke: { passed: true, smokeVersion: 9 },
    inputs: 'd'.repeat(64),
    stagedAt: '2026-11-02T00:00:00.000Z',
    files: list,
  }
  const prefix = o.top ? `${o.top}/` : ''
  if (o.tamper) files['bin/gmp.dll'] = 'MZ another gmp'
  return [
    ...(o.top ? [{ name: prefix, data: '', method: 0 }] : []),
    { name: `${prefix}manifest.json`, data: JSON.stringify(manifest, null, 2) },
    ...Object.entries(files).map(([p, d]) => ({ name: `${prefix}${p}`, data: d })),
  ]
}

interface FakeGithub {
  get: JsonGetter
  fetch: FetchLike
  asked: string[]
}

/**
 * A release of TAG with the engine zip, its source zip and SHA256SUMS. `sums` overrides what
 * SHA256SUMS says of the zip, `listedSize` the size the release lists for it.
 */
function fakeGithub(zip: Buffer, o: { sums?: string; listedSize?: number; digest?: string } = {}): FakeGithub {
  const name = 'sketch-orca-0.1.0-orca-2.4.2-win32-x64.zip'
  const sums = `${o.sums ?? sha256(zip)}  ${name}\n${'e'.repeat(64)}  sketch-orca-0.1.0-orca-2.4.2-win32-x64-source.zip\n`
  const bodies: Record<string, Buffer> = { [`${STORE}/1`]: zip, [`${STORE}/2`]: Buffer.from(sums), [`${STORE}/3`]: Buffer.from('source') }
  const asked: string[] = []
  const asset = (n: string, size: number, id: number, digest?: string) => ({ name: n, size, state: 'uploaded', browser_download_url: `${DL}/${n}`, id, ...(digest ? { digest: `sha256:${digest}` } : {}) })
  return {
    asked,
    get: async (url) => {
      asked.push(url)
      if (url !== `https://api.github.com/repos/${REPO}/releases/tags/${TAG}`) throw new Error(`unexpected ${url}`)
      return {
        tag_name: TAG,
        draft: false,
        prerelease: false,
        assets: [asset('SHA256SUMS', sums.length, 2, sha256(sums)), asset(`${name.replace('.zip', '-source.zip')}`, 6, 3), asset(name, o.listedSize ?? zip.length, 1, o.digest)],
      }
    },
    fetch: async (url) => {
      asked.push(url)
      const m = /\/([^/]+)$/.exec(url)![1]!
      if (url.startsWith(DL)) return new Response(null, { status: 302, headers: { location: `${STORE}/${m === 'SHA256SUMS' ? 2 : m.endsWith('-source.zip') ? 3 : 1}` } })
      const body = bodies[url]
      return body ? new Response(body, { headers: { 'content-length': String(body.length) } }) : new Response('missing', { status: 404 })
    },
  }
}

function pinFor(zip: Buffer, over: Partial<ReferencePin> = {}): ReferencePin {
  return { version: '0.1.0', tag: TAG, asset: 'sketch-orca-0.1.0-orca-2.4.2-win32-x64.zip', size: zip.length, sha256: sha256(zip), stageVersion: 3, platform: 'win32', arch: 'x64', ...over }
}

let work: string
let paths: { reference: string }
const logs: string[] = []
const log = (m: string) => logs.push(m)

beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), 'orca-parity-ref-'))
  paths = { reference: join(work, 'parity', 'reference') }
  logs.length = 0
})
afterEach(() => rmSync(work, { recursive: true, force: true }))

describe('parity reference', () => {
  test('the pinned reference is the published 0.1.0 engine zip of this repository, never its source zip', () => {
    const pin = REFERENCES[DEFAULT_REFERENCE]!
    expect(DEFAULT_REFERENCE).toBe('0.1.0')
    expect(SKETCH_ORCA_REPO).toBe('typetwospace/sketch-orca')
    expect(pin).toMatchObject({ version: '0.1.0', tag: 'v0.1.0', asset: 'sketch-orca-0.1.0-orca-2.4.2-win32-x64.zip', size: 40_813_895, stageVersion: 3, platform: 'win32', arch: 'x64' })
    expect(pin.sha256).toMatch(/^[0-9a-f]{64}$/)
    expect(pin.asset).not.toContain('source')
  })

  test('downloads SHA256SUMS and the engine zip (not the source zip), checks them and unpacks the engine', async () => {
    const zip = writeZip(engineEntries())
    const gh = fakeGithub(zip, { digest: sha256(zip) })
    const r = await fetchReference({ pin: pinFor(zip), paths, get: gh.get, fetch: gh.fetch, repo: REPO, log })
    const { dir, record, downloads } = referenceDir(paths, '0.1.0')
    expect(r).toMatchObject({ dir, kept: false })
    expect(r.manifest.engine.version).toBe('0.1.0')
    expect(readFileSync(join(dir, 'bin', 'sketch-orca.exe'), 'utf8')).toBe('MZ engine')
    expect(readdirSync(paths.reference).sort()).toEqual(['0.1.0', '0.1.0.json', 'downloads'])
    expect(readdirSync(downloads).sort()).toEqual(['SHA256SUMS', 'sketch-orca-0.1.0-orca-2.4.2-win32-x64.zip'])
    expect(gh.asked.some((u) => u.includes('-source.zip'))).toBe(false)
    expect(gh.asked.filter((u) => u.startsWith(DL))).toEqual([`${DL}/SHA256SUMS`, `${DL}/sketch-orca-0.1.0-orca-2.4.2-win32-x64.zip`])
    const rec = JSON.parse(readFileSync(record, 'utf8'))
    expect(rec).toMatchObject({ format: 'sketch-orca-parity-reference/1', version: '0.1.0', tag: TAG, repo: REPO, sha256: sha256(zip), size: zip.length, url: `${DL}/sketch-orca-0.1.0-orca-2.4.2-win32-x64.zip` })
    // The signed storage address is not recorded.
    expect(readFileSync(record, 'utf8')).not.toContain(STORE)
  })

  test('a second run checks what is there and downloads nothing', async () => {
    const zip = writeZip(engineEntries())
    const gh = fakeGithub(zip)
    await fetchReference({ pin: pinFor(zip), paths, get: gh.get, fetch: gh.fetch, repo: REPO, log })
    const again = fakeGithub(zip)
    const r = await fetchReference({ pin: pinFor(zip), paths, get: again.get, fetch: again.fetch, repo: REPO, log })
    expect(r.kept).toBe(true)
    expect(again.asked).toEqual([])
    expect(keptReference(paths, pinFor(zip))?.dir).toBe(referenceDir(paths, '0.1.0').dir)
  })

  test('an unpacked file that changed is unpacked again, from the zip already downloaded', async () => {
    const zip = writeZip(engineEntries())
    await fetchReference({ pin: pinFor(zip), paths, ...fakeGithub(zip), repo: REPO, log })
    const { dir } = referenceDir(paths, '0.1.0')
    writeFileSync(join(dir, 'bin', 'gmp.dll'), 'changed')
    expect(keptReference(paths, pinFor(zip))).toBeUndefined()
    const gh = fakeGithub(zip)
    const r = await fetchReference({ pin: pinFor(zip), paths, get: gh.get, fetch: gh.fetch, repo: REPO, log })
    expect(r.kept).toBe(false)
    expect(readFileSync(join(dir, 'bin', 'gmp.dll'), 'utf8')).toBe('MZ gmp')
    expect(gh.asked.filter((u) => u.endsWith('.zip'))).toEqual([])
  })

  test('refuses a SHA256SUMS that lists another SHA-256, a release that lists another size, and a GitHub digest that disagrees', async () => {
    const zip = writeZip(engineEntries())
    await expect(fetchReference({ pin: pinFor(zip), paths, ...fakeGithub(zip, { sums: 'f'.repeat(64) }), repo: REPO, log })).rejects.toThrow(/SHA256SUMS of v0.1.0 gives .* the SHA-256 f{64}, the pin/)
    await expect(fetchReference({ pin: pinFor(zip), paths, ...fakeGithub(zip, { listedSize: zip.length + 1 }), repo: REPO, log })).rejects.toThrow(/the release lists \d+ bytes, the pin/)
    await expect(fetchReference({ pin: pinFor(zip), paths, ...fakeGithub(zip, { digest: 'a'.repeat(64) }), repo: REPO, log })).rejects.toThrow(/GitHub lists SHA-256 a{64}, the pin/)
    expect(existsSync(referenceDir(paths, '0.1.0').dir)).toBe(false)
    expect(existsSync(referenceDir(paths, '0.1.0').record)).toBe(false)
  })

  test('a zip that is not the pinned one is refused, and nothing is unpacked', async () => {
    const zip = writeZip(engineEntries())
    const other = writeZip(engineEntries({ version: '0.1.1' }))
    // SHA256SUMS lists the zip served, but the pin names another.
    await expect(fetchReference({ pin: pinFor(zip, { size: other.length }), paths, ...fakeGithub(other), repo: REPO, log })).rejects.toThrow(/the pin/)
    expect(readdirSync(paths.reference).filter((n) => n !== 'downloads')).toEqual([])
  })

  test('the unpacked engine must be what its manifest lists, at the pinned stage and engine versions', async () => {
    for (const [entries, error] of [
      [engineEntries({ tamper: true }), /changed bin\/gmp.dll/],
      [engineEntries({ stageVersion: 2 }), /stage version 2, the pin 3/],
      [engineEntries({ version: '0.1.1' }), /engine version 0.1.1, the pin 0.1.0/],
    ] as const) {
      const zip = writeZip([...entries])
      await expect(fetchReference({ pin: pinFor(zip), paths, ...fakeGithub(zip), repo: REPO, log })).rejects.toThrow(error)
      expect(readdirSync(paths.reference).filter((n) => n !== 'downloads')).toEqual([])
      rmSync(paths.reference, { recursive: true, force: true })
    }
  })

  test('a zip with the engine in one top folder is unpacked from that folder', async () => {
    const zip = writeZip(engineEntries({ top: 'sketch-orca-0.1.0-orca-2.4.2-win32-x64' }))
    const r = await fetchReference({ pin: pinFor(zip), paths, ...fakeGithub(zip), repo: REPO, log })
    expect(existsSync(join(r.dir, 'manifest.json'))).toBe(true)
    expect(readdirSync(r.dir).sort()).toEqual(['LICENSES', 'bin', 'manifest.json', 'resources'])
  })

  test('unpackZip refuses entries outside the folder and names given twice in any letter case', () => {
    const dest = join(work, 'u')
    for (const name of ['../x.txt', '/abs.txt', 'C:/x.txt', 'a\\b.txt', 'a/./b.txt', 'a//b.txt']) {
      expect(() => unpackZip(writeZip([{ name, data: 'x' }]), dest)).toThrow(/not a relative path inside the folder/)
    }
    expect(() => unpackZip(writeZip([{ name: 'A.txt', data: 'x' }, { name: 'a.txt', data: 'y' }]), dest)).toThrow(/there twice/)
    expect(unpackZip(writeZip([{ name: 'd/', data: '', method: 0 }, { name: 'd/e.txt', data: 'e' }]), dest)).toEqual(['d/e.txt'])
    expect(() => engineRoot(dest)).toThrow(/no manifest.json/)
  })
})
