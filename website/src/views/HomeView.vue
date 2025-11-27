<script setup lang="ts">
import { useHead } from '@vueuse/head'
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import BpmDemo from '../components/BpmDemo.vue'
import releaseSnapshot from '../generated/release.json'

const releaseApi = 'https://api.github.com/repos/coderDJing/bpm-sniffer/releases/latest'
const fallbackUrl = 'https://github.com/coderDJing/bpm-sniffer/releases/latest'

type ReleaseSeed = {
  tagName: string
  publishedAt: string
  notes: string
  assetName: string
  downloadUrl: string
  releaseUrl: string
  state: 'loading' | 'ready' | 'error'
}

const normalizeSnapshot = (snapshot: any): ReleaseSeed => {
  const tagName = typeof snapshot?.tagName === 'string' ? snapshot.tagName : ''
  const publishedAt = typeof snapshot?.publishedAt === 'string' ? snapshot.publishedAt : ''
  const notes = typeof snapshot?.notes === 'string' ? snapshot.notes : ''
  const downloadUrlCandidate = typeof snapshot?.downloadUrl === 'string' ? snapshot.downloadUrl : ''
  const releaseUrlCandidate = typeof snapshot?.releaseUrl === 'string' ? snapshot.releaseUrl : ''
  const assetName = typeof snapshot?.assetName === 'string' ? snapshot.assetName : ''
  const stateCandidate = snapshot?.state === 'error' ? 'error' : snapshot?.state === 'ready' ? 'ready' : undefined
  return {
    tagName,
    publishedAt,
    notes,
    assetName,
    downloadUrl: downloadUrlCandidate || fallbackUrl,
    releaseUrl: releaseUrlCandidate || fallbackUrl,
    state: stateCandidate ?? (tagName ? 'ready' : 'loading')
  }
}

const releaseSeed = normalizeSnapshot(releaseSnapshot)

const latestVersion = ref(releaseSeed.tagName)
const releaseDate = ref(releaseSeed.publishedAt)
const releaseNotes = ref(releaseSeed.notes)
const downloadUrl = ref(releaseSeed.downloadUrl)
const releaseUrl = ref(releaseSeed.releaseUrl)
const releaseAssetName = ref(releaseSeed.assetName)
const releaseState = ref<'loading' | 'ready' | 'error'>(releaseSeed.state)

async function fetchLatestRelease() {
  releaseState.value = 'loading'
  try {
    const headers: Record<string, string> = { Accept: 'application/vnd.github+json' }
    if (import.meta.env.VITE_GITHUB_TOKEN) {
      headers.Authorization = `Bearer ${import.meta.env.VITE_GITHUB_TOKEN}`
    }
    const res = await fetch(releaseApi, { headers })
    if (!res.ok) throw new Error('request failed')
    const data = await res.json()
    latestVersion.value = data.tag_name || ''
    releaseDate.value = data.published_at || ''
    releaseNotes.value = data.body || ''
    releaseUrl.value = data.html_url || fallbackUrl
    const exeAsset = (data.assets || []).find((asset: any) => /\.exe$/i.test(asset.name || ''))
    const preferredAsset = exeAsset || (data.assets || [])[0] || null
    if (preferredAsset) {
      downloadUrl.value = preferredAsset.browser_download_url || fallbackUrl
      releaseAssetName.value = preferredAsset.name || ''
    } else {
      downloadUrl.value = fallbackUrl
      releaseAssetName.value = ''
    }
    releaseState.value = 'ready'
  } catch (err) {
    console.warn('[release]', err)
    releaseState.value = 'error'
    downloadUrl.value = fallbackUrl
  }
}

const releaseNotesExcerpt = computed(() => {
  if (!releaseNotes.value) return []
  return releaseNotes.value
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith('#'))
    .slice(0, 3)
})

const SITE_NAME = 'BPM Sniffer'
const FALLBACK_SITE_URL = 'https://coderDJing.github.io/bpm-sniffer/'
const envSiteUrlRaw = (import.meta.env as Record<string, string | undefined>).VITE_SITE_URL
const envSiteUrl = envSiteUrlRaw ? envSiteUrlRaw.trim() : ''
const ensureTrailingSlash = (value: string) => {
  if (!value) return '/'
  return value.endsWith('/') ? value : `${value}/`
}
const staticSiteBase = ensureTrailingSlash(envSiteUrl || FALLBACK_SITE_URL)

const route = useRoute()
const router = useRouter()

const LANG_STORAGE_KEY = 'bpm_site_lang'
type SiteLang = 'zh' | 'en'
type FeatureEntry = { title: string; detail: string }
type StepEntry = { label: string; title: string; detail: string }
type DemoEntry = {
  metaText: string
  confText: string
  refresh: string
  themeToggle: string
  pinOn: string
  pinOff: string
  floatingOn: string
  floatingOff: string
}

type SeoEntry = {
  title: string
  description: string
  keywords: string[]
  locale: string
}

type TranslationEntry = {
  eyebrow: string
  heroTitle: string
  heroLede: string
  heroSecondaryCta: string
  downloadDefault: string
  downloadWithVersion: (version: string) => string
  releaseLatestPrefix: string
  releaseDatePrefix: string
  releaseLinkReady: string
  releaseLoadingLink: string
  releaseError: string
  releaseNotesTitle: (version?: string) => string
  featuresTitle: string
  features: FeatureEntry[]
  stepsTitle: string
  steps: StepEntry[]
  langToggleLabel: string
  demo: DemoEntry
  seo: SeoEntry
}

const translations: Record<SiteLang, TranslationEntry> = {
  zh: {
    eyebrow: 'System audio · Real-time BPM · Zero driver',
    heroTitle: 'BPM Sniffer，让任何音源都能秒出 BPM：轻量、开源、无需导入，一开即测。',
    heroLede:
      'Windows 10+ 上即装即用的 BPM 侦测工具。自动监听系统播放的音乐，保持稳定数值、绚烂可视化，并通过 OTA 更新始终保持最佳手感。',
    heroSecondaryCta: '浏览 GitHub',
    downloadDefault: '下载最新正式版',
    downloadWithVersion: (version: string) => `立即下载 ${version}`,
    releaseLatestPrefix: '最新稳定版 · ',
    releaseDatePrefix: '发布于 ',
    releaseLinkReady: 'Release Note',
    releaseLoadingLink: '获取最新版本',
    releaseError: '无法从 GitHub 读取最新版本，按钮仍将跳转到 Releases 页面。',
    releaseNotesTitle: (version?: string) => `${version || '最新版本'} 更新摘要`,
    featuresTitle: '为每一个需要快速知晓 BPM 的人打造',
    features: [
      { title: '系统回环捕获', detail: '无需虚拟声卡或麦克风权限，安装即用，兼容绝大多数播放器与浏览器音源。' },
      { title: '稳定视觉反馈', detail: '浮动窗口与多种波形可视化与主题同步，无论何种音源都能随时掌握当前 BPM。' },
      { title: 'OTA 自动更新', detail: 'GitHub 稳定版支持 OTA，预发布版本由你手动体验，保证生产环境的稳定。' },
      { title: '完全本地化', detail: '全流程离线处理，无需上传音频，配合中英双语界面，适配各种场景。' }
    ],
    stepsTitle: '开始使用 BPM Sniffer',
    steps: [
      { label: '01', title: '下载', detail: '点击“立即下载”跳转到最新正式版资产，或直接访问 GitHub Releases。' },
      { label: '02', title: '安装', detail: '运行安装包，按照向导选择路径或快捷方式，几秒即可部署完成。' },
      { label: '03', title: '播放即测', detail: '打开任意播放器或网页播放音乐，BPM Sniffer 会自动捕获并显示 BPM。' }
    ],
    langToggleLabel: 'English',
    demo: {
      metaText: '节拍稳定 · 置信度：',
      confText: '稳定',
      refresh: '刷新',
      themeToggle: '切换主题',
      pinOn: '已置顶',
      pinOff: '置顶',
      floatingOn: '双击返回',
      floatingOff: '悬浮球'
    },
    seo: {
      title: 'BPM Sniffer · 系统音频实时 BPM 侦测工具',
      description:
        'BPM Sniffer 是一款面向 Windows 10+ 的系统音频 BPM 检测工具，安装即用、零驱动依赖，提供稳定数值、可视化与 OTA 更新。',
      keywords: ['BPM Sniffer', 'BPM 检测', '节拍侦测', '系统音频', 'DJ 工具'],
      locale: 'zh_CN'
    }
  },
  en: {
    eyebrow: 'System audio · Real-time BPM · Zero driver',
    heroTitle: 'BPM Sniffer makes any audio source report its BPM instantly—lightweight, open source, no import required.',
    heroLede:
      'A plug-and-play BPM detector for Windows 10+. It listens to whatever your system plays, keeps the numbers steady with vivid visuals, and stays sharp through OTA updates.',
    heroSecondaryCta: 'View on GitHub',
    downloadDefault: 'Download the latest release',
    downloadWithVersion: (version: string) => `Download ${version}`,
    releaseLatestPrefix: 'Latest release · ',
    releaseDatePrefix: 'Published on ',
    releaseLinkReady: 'Release notes',
    releaseLoadingLink: 'Check latest release',
    releaseError: 'Unable to fetch the latest release from GitHub. The button still opens the Releases page.',
    releaseNotesTitle: (version?: string) => `${version || 'Latest'} highlights`,
    featuresTitle: 'Built for anyone who needs to know the BPM instantly',
    features: [
      {
        title: 'System loopback capture',
        detail: 'No virtual sound card or mic permission required. Works with players, browsers, and any system audio.'
      },
      {
        title: 'Consistent visual feedback',
        detail: 'Floating window plus multiple visualizers stay in sync with your theme so you always see the current BPM.'
      },
      {
        title: 'OTA updates',
        detail: 'Stable releases update over the air, while preview builds let you try new features without risking your rig.'
      },
      { title: 'Fully local', detail: 'Everything runs offline, keeps audio on your device, and ships with bilingual UI.' }
    ],
    stepsTitle: 'Getting started with BPM Sniffer',
    steps: [
      { label: '01', title: 'Download', detail: 'Use the “Download now” button or head to GitHub Releases for the latest installer.' },
      { label: '02', title: 'Install', detail: 'Run the installer, choose shortcuts if you like, and finish setup within seconds.' },
      { label: '03', title: 'Play anything', detail: 'Start any player or web audio—BPM Sniffer automatically locks on and shows the live BPM.' }
    ],
    langToggleLabel: '中文',
    demo: {
      metaText: 'Beat locked · Confidence:',
      confText: 'Stable',
      refresh: 'Refresh',
      themeToggle: 'Toggle theme',
      pinOn: 'Pinned',
      pinOff: 'Pin window',
      floatingOn: 'Double-tap to return',
      floatingOff: 'Floating widget'
    },
    seo: {
      title: 'BPM Sniffer · Real-time system audio BPM detector',
      description:
        'BPM Sniffer is a lightweight Windows BPM detector that listens to any system audio, keeps the BPM steady with visuals, and updates itself over the air.',
      keywords: ['BPM Sniffer', 'BPM detector', 'beat detection', 'system audio', 'DJ tool'],
      locale: 'en_US'
    }
  }
}

const routeLang = computed<SiteLang | null>(() => {
  const metaLang = route.meta.lang
  if (metaLang === 'zh' || metaLang === 'en') return metaLang
  const path = route.path?.toLowerCase?.() ?? ''
  if (path.startsWith('/en')) return 'en'
  if (path.startsWith('/zh')) return 'zh'
  return null
})

const detectDefaultLang = (): SiteLang => {
  if (routeLang.value) return routeLang.value
  if (typeof window === 'undefined') return 'zh'
  try {
    const stored = window.localStorage?.getItem(LANG_STORAGE_KEY)
    if (stored === 'zh' || stored === 'en') return stored
  } catch {
    /* ignore */
  }
  const browser = typeof navigator !== 'undefined' ? navigator.language || navigator.languages?.[0] : ''
  return browser && browser.toLowerCase().startsWith('zh') ? 'zh' : 'en'
}

const lang = ref<SiteLang>(detectDefaultLang())

watch(routeLang, (next) => {
  if (next && next !== lang.value) {
    lang.value = next
  }
})

const localized = computed(() => translations[lang.value])
const features = computed(() => localized.value.features)
const steps = computed(() => localized.value.steps)
const releaseNotesTitleText = computed(() => localized.value.releaseNotesTitle(latestVersion.value))
const releaseLinkLabel = computed(() =>
  releaseState.value === 'ready' ? localized.value.releaseLinkReady : localized.value.releaseLoadingLink
)
const downloadCta = computed(() => {
  const locale = localized.value
  if (latestVersion.value) return locale.downloadWithVersion(latestVersion.value)
  return locale.downloadDefault
})
const releaseDateText = computed(() => {
  if (!releaseDate.value) return ''
  try {
    const localeCode = lang.value === 'zh' ? 'zh-CN' : 'en-US'
    return new Date(releaseDate.value).toLocaleDateString(localeCode, {
      year: 'numeric',
      month: 'short',
      day: 'numeric'
    })
  } catch {
    return ''
  }
})
const demoI18n = computed(() => localized.value.demo)
const seoMeta = computed(() => localized.value.seo)

function navigateToLang(target: SiteLang) {
  const targetPath = target === 'en' ? '/en/' : '/'
  if (route.path !== targetPath) {
    router.replace(targetPath).catch(() => {
      /* ignore navigation duplication */
    })
  }
}

function toggleLang() {
  const next = lang.value === 'zh' ? 'en' : 'zh'
  navigateToLang(next)
}

const resolveWindowUrl = () => {
  if (typeof window === 'undefined') return ''
  const { origin, pathname } = window.location
  let normalizedPath = pathname
  if (normalizedPath.endsWith('index.html')) {
    normalizedPath = normalizedPath.slice(0, -'index.html'.length)
  }
  if (!normalizedPath.endsWith('/')) {
    normalizedPath = `${normalizedPath}/`
  }
  return `${origin}${normalizedPath}`
}

const canonicalUrl = computed(() => {
  if (!import.meta.env.SSR && typeof window !== 'undefined' && !envSiteUrl) {
    return ensureTrailingSlash(resolveWindowUrl() || '/')
  }
  const currentPath = route.path && route.path !== '/' ? route.path.replace(/^\//, '') : ''
  return ensureTrailingSlash(`${staticSiteBase}${currentPath}`)
})
const canonicalBase = computed(() => canonicalUrl.value.replace(/\/$/, ''))
const socialCardUrl = computed(() =>
  canonicalBase.value ? `${canonicalBase.value}/social-card.png` : `${canonicalUrl.value}social-card.png`
)
const alternateLinks = computed(() => [
  { rel: 'alternate', hreflang: 'zh', href: staticSiteBase },
  { rel: 'alternate', hreflang: 'en', href: `${staticSiteBase}en/` },
  { rel: 'alternate', hreflang: 'x-default', href: staticSiteBase }
])

useHead(() => {
  const meta = seoMeta.value
  const keywords = (meta.keywords || []).join(', ')
  const metaEntries = [
    { name: 'description', content: meta.description },
    keywords ? { name: 'keywords', content: keywords } : null,
    { property: 'og:title', content: meta.title },
    { property: 'og:description', content: meta.description },
    { property: 'og:locale', content: meta.locale },
    { property: 'og:url', content: canonicalUrl.value },
    { property: 'og:image', content: socialCardUrl.value },
    { property: 'og:site_name', content: SITE_NAME },
    { name: 'twitter:card', content: 'summary_large_image' },
    { name: 'twitter:title', content: meta.title },
    { name: 'twitter:description', content: meta.description },
    { name: 'twitter:image', content: socialCardUrl.value }
  ].filter((entry): entry is { name?: string; property?: string; content: string } => Boolean(entry))

  return {
    title: meta.title,
    htmlAttrs: {
      lang: lang.value
    },
    meta: metaEntries,
    link: [
      { rel: 'canonical', href: canonicalUrl.value },
      ...alternateLinks.value
    ]
  }
})
watch(lang, (val, prev) => {
  if (val === prev) return
  if (typeof window !== 'undefined') {
    try {
      window.localStorage?.setItem(LANG_STORAGE_KEY, val)
    } catch {
      /* ignore */
    }
  }
  if (routeLang.value !== val) {
    navigateToLang(val)
  }
})

onMounted(() => {
  fetchLatestRelease()
})
</script>

<template>
  <main class="page">
    <section class="hero fade-up" style="animation-delay: 0s">
      <div class="hero-top">
        <button class="lang-toggle" type="button" @click="toggleLang">
          {{ localized.langToggleLabel }}
        </button>
      </div>
      <p class="eyebrow">{{ localized.eyebrow }}</p>
      <h1>{{ localized.heroTitle }}</h1>
      <p class="lede">
        {{ localized.heroLede }}
      </p>
      <div class="hero-actions">
        <a class="btn primary" :href="downloadUrl" target="_blank" rel="noreferrer noopener">
          {{ downloadCta }}
        </a>
        <a class="btn ghost" href="https://github.com/coderDJing/bpm-sniffer" target="_blank" rel="noreferrer noopener">
          {{ localized.heroSecondaryCta }}
        </a>
      </div>
      <div
        class="release-card"
        :class="{
          warn: releaseState === 'error',
          placeholder: releaseState === 'loading'
        }"
      >
        <template v-if="releaseState === 'ready'">
          <div>
            <p>{{ localized.releaseLatestPrefix }}{{ latestVersion }}</p>
            <small v-if="releaseDateText">{{ localized.releaseDatePrefix }}{{ releaseDateText }}</small>
          </div>
          <div>
            <a :href="releaseUrl" target="_blank" rel="noreferrer noopener">{{ releaseLinkLabel }}</a>
            <span v-if="releaseAssetName"> · {{ releaseAssetName }}</span>
          </div>
        </template>
        <template v-else-if="releaseState === 'error'">
          <p>{{ localized.releaseError }}</p>
        </template>
        <template v-else>
          <div class="skeleton-line"></div>
          <div class="skeleton-line short"></div>
        </template>
      </div>
    </section>

    <section class="showcase fade-up" style="animation-delay: 0.12s">
      <BpmDemo :i18n="demoI18n" />
    </section>

    <section class="features fade-up" style="animation-delay: 0.18s">
      <h2>{{ localized.featuresTitle }}</h2>
      <div class="grid">
        <article v-for="feature in features" :key="feature.title">
          <h3>{{ feature.title }}</h3>
          <p>{{ feature.detail }}</p>
        </article>
      </div>
    </section>

    <section class="steps fade-up" style="animation-delay: 0.24s">
      <h2>{{ localized.stepsTitle }}</h2>
      <div class="step-grid">
        <article v-for="step in steps" :key="step.label">
          <span class="index">{{ step.label }}</span>
          <h3>{{ step.title }}</h3>
          <p>{{ step.detail }}</p>
        </article>
      </div>
      <div class="release-notes" v-if="releaseNotesExcerpt.length">
        <h3>{{ releaseNotesTitleText }}</h3>
        <ul>
          <li v-for="line in releaseNotesExcerpt" :key="line">{{ line }}</li>
        </ul>
      </div>
    </section>
  </main>
</template>
