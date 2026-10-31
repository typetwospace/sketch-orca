import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { parseLatestRelease, parseTagObject, parseTagRef, ReleaseError, resolveLatestRelease, type JsonGetter } from '../scripts/lib/github.ts'

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
