// 轻量 i18n：根据系统语言在简体中文与英文间切换

export type SupportedLang = 'zh-CN' | 'en'

function detectLang(): SupportedLang {
  try {
    const navLang = (navigator.language || (navigator as any).userLanguage || '').toLowerCase()
    if (navLang.startsWith('zh') && (navLang.includes('cn') || navLang.includes('hans') || navLang === 'zh')) {
      return 'zh-CN'
    }
  } catch {}
  return 'en'
}

let currentLang: SupportedLang = detectLang()

export function setLang(lang: SupportedLang) {
  currentLang = lang
}

type Dict = Record<string, { 'zh-CN': string, en: string }>

const dict: Dict = {
  app_title: { 'zh-CN': 'BPM', en: 'BPM' },
  state_tracking: { 'zh-CN': '节拍稳定', en: 'Stable' },
  state_analyzing: { 'zh-CN': '分析中', en: 'Analyzing' },
  state_uncertain: { 'zh-CN': '节拍不稳', en: 'Unstable' },
  conf_label: { 'zh-CN': '置信度：', en: 'Confidence: ' },
  conf_stable: { 'zh-CN': '稳定', en: 'High' },
  conf_medium: { 'zh-CN': '较稳', en: 'Med' },
  conf_unstable: { 'zh-CN': '不稳', en: 'Low' },
  theme_toggle_to_light: { 'zh-CN': '切换为日间模式', en: 'Switch to light mode' },
  theme_toggle_to_dark: { 'zh-CN': '切换为夜间模式', en: 'Switch to dark mode' },
  pin_on: { 'zh-CN': '已置顶', en: 'Pinned' },
  pin_title_on: { 'zh-CN': '已置顶（点击取消）', en: 'Pinned (click to unpin)' },
  pin_title_off: { 'zh-CN': '置顶', en: 'Pin on top' },
  about_title: { 'zh-CN': '关于', en: 'About' },
  about_project: { 'zh-CN': '项目地址：', en: 'Project:' },
  about_author: { 'zh-CN': '作者：', en: 'Author:' },
  about_author_name: { 'zh-CN': "Coder '程序猿/DJ'", en: "Coder 'Programmer/DJ'" },
  about_contact: { 'zh-CN': '对 BPM Sniffer 有任何建议或 Booking 我演出：', en: 'Suggestions or booking me:' },
  sun_alt: { 'zh-CN': '日间', en: 'Light' },
  moon_alt: { 'zh-CN': '夜间', en: 'Dark' },
  rms_tooltip: { 'zh-CN': 'RMS（均方根）是音频能量/响度的近似，越大代表越响', en: 'RMS approximates audio energy/loudness; higher means louder' },
}

export function t(key: keyof typeof dict): string {
  const item = dict[key]
  if (!item) return key as string
  return item[currentLang]
}

export function tn(
  zhCN: string,
  en: string
): string {
  return currentLang === 'zh-CN' ? zhCN : en
}


