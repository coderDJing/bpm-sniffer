import { defineConfig, loadEnv } from 'vite'
import vue from '@vitejs/plugin-vue'

const normalizeBase = (input) => {
  if (!input) return '/'
  let base = String(input).trim()
  if (!base) return '/'
  if (/^https?:\/\//i.test(base)) {
    try {
      const url = new URL(base)
      base = url.pathname || '/'
    } catch {
      base = '/'
    }
  }
  if (!base.startsWith('/')) base = `/${base}`
  if (!base.endsWith('/')) base = `${base}/`
  return base
}

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), '')
  const explicitBase = env.VITE_SITE_BASE && env.VITE_SITE_BASE.trim().length ? normalizeBase(env.VITE_SITE_BASE) : null
  const repoSegment = process.env.GITHUB_REPOSITORY?.split('/')?.[1]
  const repoBase = repoSegment ? normalizeBase(`/${repoSegment}`) : '/'
  const base = explicitBase ?? (process.env.GITHUB_ACTIONS ? repoBase : '/')

  return {
    plugins: [vue()],
    base
  }
})
