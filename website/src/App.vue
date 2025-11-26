<script setup>
import { computed, onMounted, ref } from 'vue'
import BpmDemo from './components/BpmDemo.vue'

const releaseApi = 'https://api.github.com/repos/coderDJing/bpm-sniffer/releases/latest'
const fallbackUrl = 'https://github.com/coderDJing/bpm-sniffer/releases/latest'

const latestVersion = ref('')
const releaseDate = ref('')
const releaseNotes = ref('')
const downloadUrl = ref(fallbackUrl)
const releaseUrl = ref(fallbackUrl)
const releaseAssetName = ref('')
const releaseState = ref('loading')

async function fetchLatestRelease() {
  releaseState.value = 'loading'
  try {
    const res = await fetch(releaseApi, {
      headers: { Accept: 'application/vnd.github+json' }
    })
    if (!res.ok) throw new Error('请求失败')
    const data = await res.json()
    latestVersion.value = data.tag_name || ''
    releaseDate.value = data.published_at || ''
    releaseNotes.value = data.body || ''
    releaseUrl.value = data.html_url || fallbackUrl
    const exeAsset = (data.assets || []).find((asset) =>
      /\.exe$/i.test(asset.name || '')
    )
    const preferredAsset =
      exeAsset || (data.assets || [])[0] || null
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

onMounted(() => {
  fetchLatestRelease()
})

const downloadCta = computed(() => {
  if (latestVersion.value) {
    return `立即下载 ${latestVersion.value}`
  }
  return '下载最新正式版'
})

const releaseDateText = computed(() => {
  if (!releaseDate.value) return ''
  try {
    return new Date(releaseDate.value).toLocaleDateString('zh-CN', {
      year: 'numeric',
      month: 'short',
      day: 'numeric'
    })
  } catch {
    return ''
  }
})

const releaseNotesExcerpt = computed(() => {
  if (!releaseNotes.value) return []
  return releaseNotes.value
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith('#'))
    .slice(0, 3)
})

const features = [
  {
    title: '系统回环捕获',
    detail: '无需虚拟声卡或麦克风权限，安装即用，兼容绝大多数播放器与浏览器音源。'
  },
  {
    title: '稳定视觉反馈',
    detail: '浮动窗口、三种波形可视化与主题同步，让你在演出或 DJ 套路中随时掌握当前 BPM。'
  },
  {
    title: 'OTA 自动更新',
    detail: 'GitHub 稳定版使用 OTA，预发布版本由你手动体验，保证生产演出环境的稳定。'
  },
  {
    title: '完全本地化',
    detail: '全流程离线处理，无需上传音频，配合中/英双语界面，适配全球舞池。'
  }
]

const steps = [
  {
    label: '01',
    title: '下载',
    detail: '点击“立即下载”跳转到最新正式版资产，或直接访问 GitHub Releases。'
  },
  {
    label: '02',
    title: '安装',
    detail: '运行 NSIS 安装包，选择是否创建桌面快捷方式，几秒即可部署完成。'
  },
  {
    label: '03',
    title: '播放即测',
    detail: '打开任意播放器或网页播放音乐，BPM Sniffer 将自动开始捕获并显示 BPM。'
  }
]

</script>

<template>
  <main class="page">
    <section class="hero">
      <p class="eyebrow">System audio · Real-time BPM · Zero driver</p>
      <h1>把舞池节奏锁在 BPM Sniffer 的掌心</h1>
      <p class="lede">
        Windows 10+ 上即装即用的 BPM 侦测工具。自动监听系统播放的音乐，保持稳定数值、绚烂可视化，并通过 OTA
        更新始终保持最佳手感。
      </p>
      <div class="hero-actions">
        <a class="btn primary" :href="downloadUrl" target="_blank" rel="noreferrer noopener">
          {{ downloadCta }}
        </a>
        <a class="btn ghost" href="https://github.com/coderDJing/bpm-sniffer" target="_blank" rel="noreferrer noopener">
          浏览 GitHub
        </a>
      </div>
      <div class="release-card" v-if="releaseState === 'ready'">
        <div>
          <p>最新稳定版 · {{ latestVersion }}</p>
          <small v-if="releaseDateText">发布于 {{ releaseDateText }}</small>
        </div>
        <div>
          <a :href="releaseUrl" target="_blank" rel="noreferrer noopener">Release Note</a>
          <span v-if="releaseAssetName"> · {{ releaseAssetName }}</span>
        </div>
      </div>
      <div v-else-if="releaseState === 'error'" class="release-card warn">
        <p>无法从 GitHub 读取最新版本，按钮仍将跳转到 Releases 页面。</p>
      </div>
    </section>

    <section class="showcase">
      <BpmDemo />
      <div class="status-panel">
        <h3>实时体验（Demo）</h3>
        <p>右侧窗口完全复刻客户端 UI，仅使用假数据驱动动画，便于在网页上提前感受交互节奏。</p>
        <ul>
          <li>
            <span>模拟 BPM</span>
            <strong>约 120–132 BPM 循环波动</strong>
          </li>
          <li>
            <span>置信度</span>
            <strong>根据假数据动态切换状态文案</strong>
          </li>
          <li>
            <span>可视化</span>
            <strong>波形 / 柱状 / 瀑布 与客户端一致</strong>
          </li>
        </ul>
        <div class="note">
          <p>真实应用会从系统回环音频读取数据，并支持浮窗、日志窗口、托盘菜单等完整功能。</p>
        </div>
      </div>
    </section>

    <section class="features">
      <h2>为 DJ、制作人和舞池工程师打造</h2>
      <div class="grid">
        <article v-for="feature in features" :key="feature.title">
          <h3>{{ feature.title }}</h3>
          <p>{{ feature.detail }}</p>
        </article>
      </div>
    </section>

    <section class="steps">
      <h2>开始使用 BPM Sniffer</h2>
      <div class="step-grid">
        <article v-for="step in steps" :key="step.label">
          <span class="index">{{ step.label }}</span>
          <h3>{{ step.title }}</h3>
          <p>{{ step.detail }}</p>
        </article>
      </div>
      <div class="release-notes" v-if="releaseNotesExcerpt.length">
        <h3>{{ latestVersion || '最新版本' }} 更新摘要</h3>
        <ul>
          <li v-for="line in releaseNotesExcerpt" :key="line">{{ line }}</li>
        </ul>
      </div>
    </section>
  </main>
</template>
