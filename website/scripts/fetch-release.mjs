import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const rootDir = path.resolve(__dirname, '..')
const outputFile = path.resolve(rootDir, 'src/generated/release.json')

const releaseApi = process.env.RELEASE_API || 'https://api.github.com/repos/coderDJing/bpm-sniffer/releases/latest'
const fallbackUrl = 'https://github.com/coderDJing/bpm-sniffer/releases/latest'

const headers = {
  'User-Agent': 'bpm-sniffer-ssg',
  Accept: 'application/vnd.github+json'
}

const token = process.env.GITHUB_TOKEN || process.env.VITE_GITHUB_TOKEN
if (token) {
  headers.Authorization = `Bearer ${token}`
}

const pickAsset = (assets = []) => {
  if (!Array.isArray(assets)) return null
  const exe = assets.find((item) => typeof item?.name === 'string' && /\.exe$/i.test(item.name))
  return exe || assets[0] || null
}

const serializeRelease = (data, source) => {
  const asset = pickAsset(data?.assets)
  return {
    tagName: data?.tag_name || '',
    publishedAt: data?.published_at || '',
    notes: data?.body || '',
    assetName: asset?.name || '',
    downloadUrl: asset?.browser_download_url || fallbackUrl,
    releaseUrl: data?.html_url || fallbackUrl,
    fetchedAt: new Date().toISOString(),
    state: data?.tag_name ? 'ready' : 'error',
    source
  }
}

const fallbackPayload = {
  tagName: '',
  publishedAt: '',
  notes: '',
  assetName: '',
  downloadUrl: fallbackUrl,
  releaseUrl: fallbackUrl,
  fetchedAt: new Date().toISOString(),
  state: 'error',
  source: 'fallback'
}

async function main() {
  let payload = fallbackPayload
  try {
    const res = await fetch(releaseApi, { headers })
    if (!res.ok) throw new Error(`GitHub API responded with ${res.status}`)
    const body = await res.json()
    payload = serializeRelease(body, 'github')
  } catch (error) {
    console.warn('[fetch-release] Failed to fetch release info, using fallback snapshot.', error)
  }

  await mkdir(path.dirname(outputFile), { recursive: true })
  await writeFile(outputFile, `${JSON.stringify(payload, null, 2)}\n`, 'utf-8')
  const displayPath = path.relative(rootDir, outputFile)
  console.log(`[fetch-release] Snapshot written to ${displayPath}`)
}

main().catch((error) => {
  console.error('[fetch-release] Unexpected error', error)
  process.exitCode = 1
})
