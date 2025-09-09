// 根据环境变量覆盖 updater 端点：仅在设置了环境端点时覆盖
// 运行时机：dev/build 之前
const fs = require('fs')
const path = require('path')

const ROOT = process.cwd()
const confPath = path.join(ROOT, 'src-tauri', 'tauri.conf.json')
const envLocalPath = path.join(ROOT, '.env.local')
const envPath = path.join(ROOT, '.env')

function readJSON(p) {
  return JSON.parse(fs.readFileSync(p, 'utf-8'))
}
function writeJSON(p, obj) {
  fs.writeFileSync(p, JSON.stringify(obj, null, 2))
}

function main() {
  if (!fs.existsSync(confPath)) return
  const conf = readJSON(confPath)
  const plugin = conf.plugins || {}
  const updater = plugin.updater || {}

  const ghFixed = 'https://github.com/coderDJing/bpm-sniffer/releases/latest/download/latest.json'
  // 读取 .env.local > .env > process.env
  const fileEnv = {}
  const loadEnvFile = (p) => {
    if (!fs.existsSync(p)) return
    const txt = fs.readFileSync(p, 'utf-8')
    for (const raw of txt.split(/\r?\n/)) {
      const line = raw.trim()
      if (!line || line.startsWith('#')) continue
      const eq = line.indexOf('=')
      if (eq <= 0) continue
      const key = line.slice(0, eq).trim()
      let val = line.slice(eq + 1).trim()
      if ((val.startsWith('"') && val.endsWith('"')) || (val.startsWith("'") && val.endsWith("'"))) {
        val = val.slice(1, -1)
      }
      if (!(key in fileEnv)) fileEnv[key] = val
    }
  }
  // 优先加载 .env.local，其次 .env
  loadEnvFile(envLocalPath)
  loadEnvFile(envPath)
  const getEnv = (k) => process.env[k] || fileEnv[k] || ''

  const envEndpoints = getEnv('UPDATER_ENDPOINTS') || getEnv('UPDATER_ENDPOINT') || ''
  const envList = envEndpoints
    .split(',')
    .map(s => s.trim())
    .filter(Boolean)

  // 策略更新：若存在环境端点，则与配置中的端点合并（去重，环境端点优先），以便多个端点共存
  if (envList.length > 0) {
    const baseList = Array.isArray(updater.endpoints) ? updater.endpoints.slice() : []
    const merged = []
    const seen = new Set()
    for (const u of envList) { if (!seen.has(u)) { seen.add(u); merged.push(u) } }
    for (const u of baseList) { if (!seen.has(u)) { seen.add(u); merged.push(u) } }
    // 如基础配置里没有 GitHub 固定端点，则追加（保证兜底）
    if (!seen.has(ghFixed)) { merged.push(ghFixed) }
    updater.endpoints = merged
    plugin.updater = updater
    conf.plugins = plugin
    writeJSON(confPath, conf)
    console.log('[prepare-updater-endpoints] endpoints merged to:', merged)
  }
}

try { main() } catch (e) { console.error('[prepare-updater-endpoints] failed:', e) }


