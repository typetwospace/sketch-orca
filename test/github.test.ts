import { describe, expect, test } from 'bun:test'
import { createHash } from 'node:crypto'
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  downloadReleaseAsset,
  isGithubDownloadUrl,
  parseLatestRelease,
  parseTaggedRelease,
  parseTagObject,
  parseTagRef,
  releaseAsset,
  ReleaseError,
  resolveLatestRelease,
  resolveTaggedRelease,
  type FetchLike,
  type JsonGetter,
  type ReleaseAsset,
} from '../scripts/lib/github.ts'

const fixture = () => JSON.parse(readFileSync(join(import.meta.dir, 'fixtures', 'github', 'release-latest.json'), 'utf8')) as Record<string, unknown>
const COMMIT = '8500fcdccaa10b5099ac20d252af3a7c560046f1'
const TAG_OBJECT = 'b'.repeat(40)
const API = 'https://api.github.com/repos/OrcaSlicer/OrcaSlicer'

describe('github', () => {
  test('parses the latest release answer (trimmed from GitHub)', () => {
    expect(parseLatestRelease(fixture())).toEqual({
      tag: 'v2.4.2',
      name: 'OrcaSlicer v2.4.2 Official Release',
      publishedAt: '2026-07-07T03:33:49Z',
      htmlUrl: 'https://github.com/OrcaSlicer/OrcaSlicer/releases/tag/v2.4.2',
    })
  })

  test('refuses drafts, pre-releases, error answers and answers without a tag', () => {
    expect(() => parseLatestRelease({ ...fixture(), draft: true })).toThrow(/is a draft/)
    expect(() => parseLatestRelease({ ...fixture(), prerelease: true })).toThrow(/pre-release/)
    expect(() => parseLatestRelease({ message: 'Not Found', documentation_url: 'https://docs.github.com' })).toThrow(/GitHub answered "Not Found"/)
    expect(() => parseLatestRelease({ ...fixture(), tag_name: '' })).toThrow(/no tag_name/)
    expect(() => parseLatestRelease([])).toThrow(ReleaseError)
  })

  test('a missing name or date becomes empty text', () => {
    const r = fixture()
    delete r.name
    delete r.published_at
    expect(parseLatestRelease(r)).toMatchObject({ name: '', publishedAt: '' })
  })

  test('tag refs: lightweight to a commit, annotated to a tag object', () => {
    expect(parseTagRef({ ref: 'refs/tags/v2.4.2', object: { type: 'commit', sha: COMMIT } }, 'v2.4.2')).toEqual({ type: 'commit', sha: COMMIT })
    expect(parseTagRef({ ref: 'refs/tags/v2.4.2', object: { type: 'tag', sha: TAG_OBJECT } }, 'v2.4.2')).toEqual({ type: 'tag', sha: TAG_OBJECT })
    expect(() => parseTagRef([{ ref: 'refs/tags/v2.4.2' }], 'v2.4.2')).toThrow(/ambiguous/)
    expect(() => parseTagRef({ ref: 'refs/tags/v2.4.20', object: { type: 'commit', sha: COMMIT } }, 'v2.4.2')).toThrow(/answer is for/)
    expect(() => parseTagRef({ ref: 'refs/tags/v2.4.2', object: { type: 'tree', sha: COMMIT } }, 'v2.4.2')).toThrow(/object type/)
    expect(() => parseTagRef({ ref: 'refs/tags/v2.4.2', object: { type: 'commit', sha: 'abc' } }, 'v2.4.2')).toThrow(/bad sha/)
    expect(parseTagObject({ sha: TAG_OBJECT, object: { type: 'commit', sha: COMMIT } })).toEqual({ type: 'commit', sha: COMMIT })
    expect(() => parseTagObject({ object: { type: 'blob', sha: COMMIT } })).toThrow(/target type/)
  })

  function getter(answers: Record<string, unknown>, asked: string[]): JsonGetter {
    return async (url) => {
      asked.push(url)
      if (!(url in answers)) throw new Error(`unexpected request ${url}`)
      return answers[url]
    }
  }

  test('resolveLatestRelease follows a lightweight tag to its commit', async () => {
    const asked: string[] = []
    const r = await resolveLatestRelease(
      'OrcaSlicer/OrcaSlicer',
      getter({ [`${API}/releases/latest`]: fixture(), [`${API}/git/ref/tags/v2.4.2`]: { ref: 'refs/tags/v2.4.2', object: { type: 'commit', sha: COMMIT } } }, asked),
    )
    expect(r).toEqual({ tag: 'v2.4.2', commit: COMMIT, publishedAt: '2026-07-07T03:33:49Z', name: 'OrcaSlicer v2.4.2 Official Release', htmlUrl: 'https://github.com/OrcaSlicer/OrcaSlicer/releases/tag/v2.4.2' })
    expect(asked).toEqual([`${API}/releases/latest`, `${API}/git/ref/tags/v2.4.2`])
  })

  test('resolveLatestRelease follows an annotated tag through its tag object', async () => {
    const asked: string[] = []
    const r = await resolveLatestRelease(
      'OrcaSlicer/OrcaSlicer',
      getter(
        {
          [`${API}/releases/latest`]: fixture(),
          [`${API}/git/ref/tags/v2.4.2`]: { ref: 'refs/tags/v2.4.2', object: { type: 'tag', sha: TAG_OBJECT } },
          [`${API}/git/tags/${TAG_OBJECT}`]: { sha: TAG_OBJECT, object: { type: 'commit', sha: COMMIT } },
        },
        asked,
      ),
    )
    expect(r.commit).toBe(COMMIT)
    expect(asked).toHaveLength(3)
  })

  test('resolveLatestRelease gives up on a loop of tag objects', async () => {
    const loop = { sha: TAG_OBJECT, object: { type: 'tag', sha: TAG_OBJECT } }
    const answers = { [`${API}/releases/latest`]: fixture(), [`${API}/git/ref/tags/v2.4.2`]: { ref: 'refs/tags/v2.4.2', object: { type: 'tag', sha: TAG_OBJECT } }, [`${API}/git/tags/${TAG_OBJECT}`]: loop }
    await expect(resolveLatestRelease('OrcaSlicer/OrcaSlicer', getter(answers, []))).rejects.toThrow(/too many nested tag objects/)
  })
})

const DL = 'https://github.com/example/tool/releases/download/v1.0.0'
const sha256 = (b: Uint8Array | string) => createHash('sha256').update(b).digest('hex')

/** A release answer as GET /repos/{repo}/releases/tags/{tag} gives it, trimmed. */
function taggedRelease(assets: Record<string, unknown>[]): Record<string, unknown> {
  return { tag_name: 'v1.0.0', name: 'tool 1.0.0', draft: false, prerelease: false, published_at: '2026-11-02T02:47:06Z', target_commitish: '0da2256baf0119baf124610ac331df2fc1008de0', assets }
}

describe('github: a tagged release and its assets', () => {
  const asset = (name: string, size: number, digest?: string) => ({ name, size, state: 'uploaded', browser_download_url: `${DL}/${name}`, ...(digest ? { digest } : {}) })

  test('parses the release of a tag with its assets, their sizes and GitHub digests', () => {
    const r = parseTaggedRelease(taggedRelease([asset('a.zip', 10, `sha256:${'a'.repeat(64)}`), asset('SHA256SUMS', 70)]), 'v1.0.0')
    expect(r).toEqual({
      tag: 'v1.0.0',
      name: 'tool 1.0.0',
      publishedAt: '2026-11-02T02:47:06Z',
      commitish: '0da2256baf0119baf124610ac331df2fc1008de0',
      assets: [
        { name: 'a.zip', size: 10, url: `${DL}/a.zip`, sha256: 'a'.repeat(64) },
        { name: 'SHA256SUMS', size: 70, url: `${DL}/SHA256SUMS` },
      ],
    })
    expect(releaseAsset(r, 'a.zip').size).toBe(10)
    expect(() => releaseAsset(r, 'b.zip')).toThrow(/no asset b\.zip/)
    expect(() => releaseAsset({ ...r, assets: [...r.assets, r.assets[0]!] }, 'a.zip')).toThrow(/more than one asset a\.zip/)
  })

  test('refuses drafts, pre-releases, another tag, assets still uploading and downloads off github.com', () => {
    expect(() => parseTaggedRelease({ ...taggedRelease([]), draft: true }, 'v1.0.0')).toThrow(/draft/)
    expect(() => parseTaggedRelease({ ...taggedRelease([]), prerelease: true }, 'v1.0.0')).toThrow(/pre-release/)
    expect(() => parseTaggedRelease(taggedRelease([]), 'v1.0.1')).toThrow(/answer is for "v1.0.0"/)
    expect(() => parseTaggedRelease({ message: 'Not Found' }, 'v1.0.0')).toThrow(/GitHub answered "Not Found"/)
    expect(() => parseTaggedRelease(taggedRelease([{ ...asset('a.zip', 1), state: 'starter' }]), 'v1.0.0')).toThrow(/not uploaded/)
    expect(() => parseTaggedRelease(taggedRelease([{ ...asset('a.zip', 1), browser_download_url: 'http://github.com/x' }]), 'v1.0.0')).toThrow(/not on https:\/\/github.com/)
    expect(() => parseTaggedRelease(taggedRelease([{ ...asset('a.zip', 1), browser_download_url: 'https://example.com/a.zip' }]), 'v1.0.0')).toThrow(/not on https:\/\/github.com/)
    expect(() => parseTaggedRelease(taggedRelease([{ ...asset('a.zip', 1), size: -1 }]), 'v1.0.0')).toThrow(/bad size/)
  })

  test('resolveTaggedRelease asks for the release of the tag', async () => {
    const asked: string[] = []
    const get: JsonGetter = async (url) => {
      asked.push(url)
      return taggedRelease([])
    }
    expect((await resolveTaggedRelease('example/tool', 'v1.0.0', get)).tag).toBe('v1.0.0')
    expect(asked).toEqual(['https://api.github.com/repos/example/tool/releases/tags/v1.0.0'])
  })

  test('isGithubDownloadUrl: https on github.com and githubusercontent.com hosts only', () => {
    expect(isGithubDownloadUrl(`${DL}/a.zip`)).toBe(true)
    expect(isGithubDownloadUrl('https://release-assets.githubusercontent.com/x?sig=1')).toBe(true)
    expect(isGithubDownloadUrl('https://objects.githubusercontent.com/x')).toBe(true)
    expect(isGithubDownloadUrl('http://github.com/a')).toBe(false)
    expect(isGithubDownloadUrl('https://github.com.example.com/a')).toBe(false)
    expect(isGithubDownloadUrl('https://evilgithubusercontent.com/a')).toBe(false)
    expect(isGithubDownloadUrl('https://user:pw@github.com/a')).toBe(false)
    expect(isGithubDownloadUrl('not a url')).toBe(false)
  })
})

describe('github: downloadReleaseAsset', () => {
  const body = new Uint8Array(Array.from({ length: 5000 }, (_, i) => (i * 7) % 256))
  const asset: ReleaseAsset = { name: 'a.zip', size: body.length, url: `${DL}/a.zip` }
  const STORE = 'https://release-assets.githubusercontent.com/a.zip?signed=1'

  /** A fake fetch: GitHub's download address redirects to its storage host, which streams `bytes` in chunks. */
  function fakeFetch(bytes: Uint8Array, o: { asked?: string[]; length?: number; status?: number; location?: string } = {}): FetchLike {
    return async (url, init) => {
      o.asked?.push(url)
      expect(init.redirect).toBe('manual')
      if (url === asset.url) return new Response(null, { status: 302, headers: { location: o.location ?? STORE } })
      if (o.status) return new Response('no', { status: o.status })
      const stream = new ReadableStream<Uint8Array>({
        start(c) {
          for (let i = 0; i < bytes.length; i += 1000) c.enqueue(bytes.slice(i, i + 1000))
          c.close()
        },
      })
      return new Response(stream, { status: 200, headers: { 'content-length': String(o.length ?? bytes.length) } })
    }
  }

  function withDir(f: (dir: string) => Promise<void>): Promise<void> {
    const dir = mkdtempSync(join(tmpdir(), 'orca-download-'))
    return f(dir).finally(() => rmSync(dir, { recursive: true, force: true }))
  }

  test('follows the redirect, checks the size and SHA-256 while streaming, and writes the file through a temporary name', () =>
    withDir(async (dir) => {
      const asked: string[] = []
      const dest = join(dir, 'sub', 'a.zip')
      const got = await downloadReleaseAsset(asset, dest, { sha256: sha256(body), fetch: fakeFetch(body, { asked }) })
      expect(got).toEqual({ path: dest, size: body.length, sha256: sha256(body), from: STORE })
      expect(asked).toEqual([asset.url, STORE])
      expect(new Uint8Array(readFileSync(dest))).toEqual(body)
      expect(readdirSync(join(dir, 'sub'))).toEqual(['a.zip'])
    }))

  test("takes GitHub's digest when no SHA-256 is given, and refuses when the two disagree or there is none", () =>
    withDir(async (dir) => {
      await downloadReleaseAsset({ ...asset, sha256: sha256(body) }, join(dir, 'a.zip'), { fetch: fakeFetch(body) })
      await expect(downloadReleaseAsset({ ...asset, sha256: 'b'.repeat(64) }, join(dir, 'b.zip'), { sha256: sha256(body), fetch: fakeFetch(body) })).rejects.toThrow(/GitHub lists SHA-256/)
      await expect(downloadReleaseAsset(asset, join(dir, 'c.zip'), { fetch: fakeFetch(body) })).rejects.toThrow(/no SHA-256 to check/)
      expect(existsSync(join(dir, 'b.zip'))).toBe(false)
    }))

  test('a wrong SHA-256, a short or long download, or a wrong length leaves nothing behind', () =>
    withDir(async (dir) => {
      const dest = join(dir, 'a.zip')
      const cases: [FetchLike, string | undefined, RegExp][] = [
        [fakeFetch(body), 'c'.repeat(64), /SHA-256 [0-9a-f]{64}, expected c{64}/],
        [fakeFetch(body.slice(0, 4000), { length: body.length }), undefined, /downloaded 4000 bytes, the release lists 5000/],
        [fakeFetch(new Uint8Array([...body, 1, 2, 3]), { length: body.length }), undefined, /passes the 5000 bytes/],
        [fakeFetch(body, { length: 12 }), undefined, /sends 12 bytes, the release lists 5000/],
        [fakeFetch(body, { status: 404 }), undefined, /HTTP 404/],
      ]
      for (const [f, want, error] of cases) {
        await expect(downloadReleaseAsset(asset, dest, { sha256: want ?? sha256(body), fetch: f })).rejects.toThrow(error)
        expect(readdirSync(dir)).toEqual([])
      }
    }))

  test('a file already at the destination is replaced only by a download that passes', () =>
    withDir(async (dir) => {
      const dest = join(dir, 'a.zip')
      writeFileSync(dest, 'old')
      await expect(downloadReleaseAsset(asset, dest, { sha256: 'c'.repeat(64), fetch: fakeFetch(body) })).rejects.toThrow(ReleaseError)
      expect(readFileSync(dest, 'utf8')).toBe('old')
      await downloadReleaseAsset(asset, dest, { sha256: sha256(body), fetch: fakeFetch(body) })
      expect(readFileSync(dest).length).toBe(body.length)
    }))

  test('refuses a redirect off GitHub, too many redirects, and a redirect without a Location', () =>
    withDir(async (dir) => {
      const dest = join(dir, 'a.zip')
      await expect(downloadReleaseAsset(asset, dest, { sha256: sha256(body), fetch: fakeFetch(body, { location: 'https://example.com/a.zip' }) })).rejects.toThrow(/refusing to download from https:\/\/example.com/)
      await expect(downloadReleaseAsset(asset, dest, { sha256: sha256(body), fetch: fakeFetch(body, { location: 'http://objects.githubusercontent.com/a' }) })).rejects.toThrow(/refusing to download/)
      const loop: FetchLike = async (url) => new Response(null, { status: 302, headers: { location: url } })
      await expect(downloadReleaseAsset(asset, dest, { sha256: sha256(body), fetch: loop, maxRedirects: 3 })).rejects.toThrow(/more than 3 redirects/)
      const bare: FetchLike = async () => new Response(null, { status: 302 })
      await expect(downloadReleaseAsset(asset, dest, { sha256: sha256(body), fetch: bare })).rejects.toThrow(/without a Location/)
      expect(readdirSync(dir)).toEqual([])
    }))
})
