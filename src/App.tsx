import React, { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import thumbtack from './assets/thumbtack.png'
import sun from './assets/sun.png'
import moon from './assets/moon.png'
// @ts-ignore: optional plugin at runtime
import { check } from '@tauri-apps/plugin-updater'

type DisplayBpm = { bpm: number, confidence: number, state: 'tracking'|'uncertain'|'analyzing', level: number }
type AudioViz = { samples: number[], rms: number }

export default function App() {
  const [bpm, setBpm] = useState<number | null>(null)
  const [conf, setConf] = useState<number | null>(null)
  const [state, setState] = useState<DisplayBpm['state']>('analyzing')
  const [alwaysOnTop, setAlwaysOnTop] = useState<boolean>(false)
  const [viz, setViz] = useState<AudioViz | null>(null)
  const [vizMode, setVizMode] = useState<'wave'|'bars'|'waterfall'>('wave')
  const [themeName, setThemeName] = useState<'dark' | 'light'>('dark')
  const mqlCleanupRef = useRef<null | (() => void)>(null)

  const darkTheme = {
    background: '#14060a',
    textPrimary: '#ffffff',
    textSecondary: '#f3a0b3',
    subduedText: '#6b829e',
    accent: '#eb1a50',
    panelBg: '#1a0a0f',
    grid: '#3a0b17',
    line: '#eb1a50',
    track: '#3a0b17',
    confGray: '#9aa3ab'
  }
  const lightTheme = {
    background: '#fff4f7',
    textPrimary: '#1b0a10',
    textSecondary: '#b21642',
    subduedText: '#5c6c7a',
    accent: '#eb1a50',
    panelBg: '#ffe8ee',
    grid: '#ffd0db',
    line: '#eb1a50',
    track: '#ffd0db',
    confGray: '#8a8f96'
  }
  const theme = themeName === 'dark' ? darkTheme : lightTheme

  useEffect(() => {
    let removeListener: (() => void) | null = null
    let removeMql: (() => void) | null = null
    ;(async () => {
      try {
        // 初始化主题：优先读取用户偏好，否则跟随系统
        try {
          const saved = localStorage.getItem('bpm_theme')
          const mql = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)')
          if (saved === 'light' || saved === 'dark') {
            setThemeName(saved as 'light' | 'dark')
          } else if (mql) {
            setThemeName(mql.matches ? 'dark' : 'light')
            const handler = (e: MediaQueryListEvent) => setThemeName(e.matches ? 'dark' : 'light')
            if (mql.addEventListener) mql.addEventListener('change', handler)
            else if ((mql as any).addListener) (mql as any).addListener(handler)
            removeMql = () => {
              if (mql.removeEventListener) mql.removeEventListener('change', handler)
              else if ((mql as any).removeListener) (mql as any).removeListener(handler)
            }
            mqlCleanupRef.current = removeMql
          }
        } catch {}

        // 恢复置顶状态 / 可视化模式（窗口大小不做持久化）
        try {
          const savedTop = localStorage.getItem('bpm_on_top')
          if (savedTop === '1' || savedTop === 'true') {
            await invoke('set_always_on_top', { onTop: true })
            setAlwaysOnTop(true)
          }
          const savedViz = localStorage.getItem('bpm_viz_mode')
          if (savedViz === 'wave' || savedViz === 'bars' || savedViz === 'waterfall') {
            setVizMode(savedViz as 'wave'|'bars'|'waterfall')
          }
        } catch {}

        await invoke('start_capture')
        // 静默检查并下载更新（可达性自动选择端点）
        try {
          const update = await check()
          if (update?.available) {
            const downloaded = await update.downloadAndInstall()
            if (downloaded) {
              // 可选：立即重启，或下次启动生效
              // await relaunch()
            }
          }
        } catch {}
        const unlistenA = await listen<DisplayBpm>('bpm_update', (e) => {
          const res = e.payload
          setConf(res.confidence)
          setState(res.state)
          // 后端已做过滤：收到即显示；为0则保留上一次
          if (res.bpm > 0) setBpm(res.bpm)
        })
        const unlistenD = await listen<AudioViz>('viz_update', (e) => {
          setViz(e.payload as any as AudioViz)
        })
        removeListener = () => { if (removeMql) removeMql(); unlistenA(); unlistenD() }
      } catch (err) { console.error('[BOOT] error', err) }
    })()

    return () => { if (removeListener) removeListener() }
  }, [])


  function toggleTheme() {
    const next = themeName === 'dark' ? 'light' : 'dark'
    setThemeName(next)
    try { localStorage.setItem('bpm_theme', next) } catch {}
    if (mqlCleanupRef.current) { mqlCleanupRef.current(); mqlCleanupRef.current = null }
  }

  const label = state === 'tracking' ? '节拍稳定' : state === 'analyzing' ? '分析中' : '节拍不稳'
  const confLabel = conf == null ? '—' : conf >= 0.75 ? '稳定' : conf >= 0.5 ? '较稳' : '不稳'
  const confColor = conf == null ? theme.confGray : (conf >= 0.5 ? theme.textPrimary : theme.confGray)
  const bpmColor = conf == null ? theme.confGray : (conf >= 0.5 ? theme.textPrimary : theme.confGray)

  // 已固定后端为基础模式，无切换

  async function toggleAlwaysOnTop() {
    try {
      const next = !alwaysOnTop
      await invoke('set_always_on_top', { onTop: next })
      setAlwaysOnTop(next)
      try { localStorage.setItem('bpm_on_top', next ? '1' : '0') } catch {}
    } catch (e) {
      console.error('置顶切换失败', e)
    }
  }

  // 自适应隐藏：根据窗口高度动态隐藏部分元素
  const [hideRms, setHideRms] = useState(false)
  const [hideViz, setHideViz] = useState(false)
  const [hideTitle, setHideTitle] = useState(false)
  const [hideMeta, setHideMeta] = useState(false)
  const [hideActions, setHideActions] = useState(false)
  // 轻量触发器：即便各隐藏标志未变化，也强制刷新以更新 VizPanel 宽度
  const [sizeTick, setSizeTick] = useState(0)
  useEffect(() => {
    function onResize() {
      const h = window.innerHeight
      const w = window.innerWidth
      // 粗略阈值：根据当前组件布局估算
      setHideRms(h < 380)
      setHideViz(h < 360)
      setHideTitle(h < 225)
      setHideMeta(h < 175)
      setHideActions(w < 310)
      // 强制一次轻量刷新，确保宽度自适应在静态画面时也更新
      setSizeTick((t) => (t + 1) % 1000000)
      console.log(`[win] ${window.innerWidth} x ${window.innerHeight}`)
    }
    onResize()
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])

  // 持久化可视化模式
  useEffect(() => {
    try { localStorage.setItem('bpm_viz_mode', vizMode) } catch {}
  }, [vizMode])

  return (
    <main style={{height:'100vh',display:'flex',flexDirection:'column',alignItems:'center',justifyContent:'center',gap:16,background:theme.background,color:theme.textPrimary,overflow:'hidden'}}>
      {!hideTitle && <h1 style={{margin:0,color:'#eb1a50',fontSize:18}}>BPM</h1>}
      <div style={{fontSize:96,fontWeight:700,letterSpacing:2,color:bpmColor}}>{bpm == null ? 0 : Math.round(bpm)}</div>
      {!hideMeta && (
        <div style={{fontSize:14,color:theme.textSecondary}}>
          {label} · 置信度：<span style={{color: confColor}}>{confLabel}</span>
        </div>
      )}

      {/* 简易波形可视化 */}
      {!hideViz && (
        <VizPanel theme={theme} hideRms={hideRms} viz={viz} mode={vizMode} onToggle={() => setVizMode(m => m==='wave' ? 'bars' : (m==='bars' ? 'waterfall' : 'wave'))} />
      )}

      {!hideActions && (
      <div style={{position:'fixed',right:12,top:12,display:'flex',gap:8,alignItems:'center'}}>
        <button
          onClick={toggleTheme}
          title={themeName === 'dark' ? '切换为日间模式' : '切换为夜间模式'}
          style={{
            background:'transparent',
            border:'none',
            padding:0,
            cursor:'pointer',
            width:25,
            height:25,
            display:'flex',
            alignItems:'center',
            justifyContent:'center'
          }}
        >
          <div style={{position:'relative', width:25, height:25}}>
            <img
              src={sun}
              alt="日间"
              width={25}
              height={25}
              style={{
                position:'absolute',
                left:0,
                top:0,
                opacity: themeName === 'dark' ? 1 : 0,
                transform: themeName === 'dark' ? 'rotate(0deg) scale(1)' : 'rotate(-90deg) scale(0.85)',
                transition:'opacity 180ms ease, transform 220ms ease'
              }}
            />
            <img
              src={moon}
              alt="夜间"
              width={25}
              height={25}
              style={{
                position:'absolute',
                left:0,
                top:0,
                opacity: themeName === 'dark' ? 0 : 1,
                transform: themeName === 'dark' ? 'rotate(90deg) scale(0.85)' : 'rotate(0deg) scale(1)',
                transition:'opacity 180ms ease, transform 220ms ease'
              }}
            />
          </div>
        </button>
        <button
          onClick={toggleAlwaysOnTop}
          title={alwaysOnTop ? '已置顶（点击取消）' : '置顶'}
          style={{
            background:'transparent',
            border:'none',
            padding:0,
            cursor:'pointer',
            width:25,
            height:25,
            display:'flex',
            alignItems:'center',
            justifyContent:'center'
          }}
        >
          <img
            src={thumbtack}
            alt={alwaysOnTop ? '已置顶' : '置顶'}
            width={25}
            height={25}
            style={{
              transform: alwaysOnTop ? 'rotate(-45deg)' : 'none',
              transition:'transform 120ms ease'
            }}
          />
        </button>
      </div>
      )}

      {/* 调试面板已移除，仅保留导出功能 */}
    </main>
  )
}

function VizPanel({ theme, hideRms, viz, mode, onToggle }: { theme: any, hideRms: boolean, viz: AudioViz | null, mode: 'wave'|'bars'|'waterfall', onToggle: () => void }) {
  const h = 120
  const w = Math.max(180, Math.floor(window.innerWidth - 10))
  const bg = theme.panelBg
  const grid = theme.grid
  const line = theme.line
  const accent = theme.accent
  const rmsRaw = viz?.rms ?? 0
  const samples = viz?.samples ?? []
  const silentCut = 0.015
  // 快速静音判定：当音量相对上一帧骤降，直接视为静音（解决“戛然而止”不归零）
  const prevRmsRef = React.useRef(0)
  const [fastSilent, setFastSilent] = React.useState(false)
  React.useEffect(() => {
    const prev = prevRmsRef.current
    const drop = prev > 0.08 && rmsRaw < prev * 0.25
    setFastSilent(drop)
    prevRmsRef.current = rmsRaw
  }, [rmsRaw])
  const isSilent = rmsRaw < silentCut || fastSilent

  // 帧间平滑：用 ref 避免 setState 导致的渲染环
  const lastSmoothedRef = React.useRef<number[] | null>(null)
  const smoothSamples = React.useMemo(() => {
    const alpha = 0.35
    const base = isSilent ? new Array(samples.length).fill(0) : samples
    if (!base.length) return [] as number[]
    const prev = lastSmoothedRef.current
    if (!prev || prev.length !== base.length) {
      lastSmoothedRef.current = base
      return base
    }
    const out = new Array(base.length)
    for (let i = 0; i < base.length; i++) {
      out[i] = prev[i] * (1 - alpha) + base[i] * alpha
    }
    lastSmoothedRef.current = out
    return out
  }, [samples, isSilent])

  // RMS 平滑（用 ref，避免触发重渲染环）
  const rmsSmoothedRef = React.useRef(0)
  React.useEffect(() => {
    const prev = rmsSmoothedRef.current
    if (isSilent) { rmsSmoothedRef.current = 0; return }
    const alphaUp = 0.15
    const alphaDown = 0.35
    const alpha = rmsRaw < prev ? alphaDown : alphaUp
    rmsSmoothedRef.current = prev * (1 - alpha) + rmsRaw * alpha
  }, [rmsRaw, isSilent])

  // 自适应增益（用 ref）
  const peakRef = React.useRef(0.3)
  React.useEffect(() => {
    if (!smoothSamples.length) return
    let localPeak = 0
    for (let i = 0; i < smoothSamples.length; i++) {
      const a = Math.abs(smoothSamples[i] || 0)
      if (a > localPeak) localPeak = a
    }
    peakRef.current = isSilent ? 0.2 : (peakRef.current * 0.95 + localPeak * 0.05)
  }, [smoothSamples, isSilent])
  // 降低基础增益，并限制上下限，取折中视觉效果
  const base = Math.max(0.12, Math.min(0.6, peakRef.current))
  let gain = 0.6 / base
  gain = Math.max(0.8, Math.min(2.2, gain))

  function renderWave() {
    const mid = Math.floor(h / 2)
    const path = smoothSamples.map((v, i) => {
      const x = Math.round((i / Math.max(1, samples.length - 1)) * (w - 1))
      const y = mid - Math.round(Math.max(-1, Math.min(1, (v || 0) * gain)) * (h * 0.40))
      return `${i === 0 ? 'M' : 'L'}${x},${y}`
    }).join(' ')
    return <>
      <line x1="0" y1={mid} x2={w} y2={mid} stroke={grid} strokeWidth="1" />
      {path && <path d={path} stroke={line} strokeWidth={1.5} fill="none" />}
    </>
  }

  function renderBars() {
    // 将 256 点折叠为 64 根柱，每根取局部绝对值均值
    const bars = 64
    const step = Math.max(1, Math.floor(smoothSamples.length / bars))
    const elems: JSX.Element[] = []
    const gap = 1
    const barW = Math.max(2, Math.floor((w - (bars - 1) * gap) / bars))
    const totalW = barW * bars + gap * (bars - 1)
    const offsetX = Math.max(0, Math.floor((w - totalW) / 2))
    for (let b = 0; b < bars; b++) {
      const i0 = b * step
      const i1 = Math.min(smoothSamples.length, i0 + step)
      let acc = 0, cnt = 0
      for (let i = i0; i < i1; i++) { acc += Math.abs(smoothSamples[i] || 0); cnt++ }
      const v = cnt ? acc / cnt : 0
      const vv = Math.min(1, v * gain)
      const x = offsetX + b * (barW + gap)
      const bh = Math.round(vv * (h - 4))
      const y = h - 2 - bh
      elems.push(<rect key={b} x={x} y={y} width={barW} height={bh} fill={line} opacity={0.85} />)
    }
    return <>{elems}</>
  }

  // Waterfall：把每帧样本折叠成 bands 段能量（历史用 ref）
  const histRef = React.useRef<number[][]>([])
  const bands = 16
  const scrollMul = 6
  const cellW = 4
  const gap = 1
  const visibleCols = Math.max(1, Math.floor((w - gap) / (cellW + gap)))
  const wfW = Math.min(w, visibleCols * (cellW + gap) + gap)
  const wfH = Math.min(h, bands * (cellW + gap) + gap)
  const wfOffsetX = Math.max(0, Math.floor((w - wfW) / 2))
  const wfOffsetY = Math.max(0, Math.floor((h - wfH) / 2))
  const maxHistory = 600
  // 将一帧样本折叠为 bands 段能量
  const frameBands = React.useMemo(() => {
    const out = new Array(bands).fill(0)
    if (!smoothSamples.length) return out
    const step = Math.max(1, Math.floor(smoothSamples.length / bands))
    for (let b = 0; b < bands; b++) {
      const i0 = b * step
      const i1 = Math.min(smoothSamples.length, i0 + step)
      let acc = 0, cnt = 0
      for (let i = i0; i < i1; i++) { acc += Math.abs(smoothSamples[i] || 0); cnt++ }
      const v = cnt ? acc / cnt : 0
      out[b] = Math.min(1, v * gain)
    }
    return out
  }, [smoothSamples, gain])
  // 维护历史（右侧为最新），按 scrollMul 倍速推进（不变稀疏：仅推进历史，不减少可见列）
  React.useEffect(() => {
    if (!frameBands.length) return
    let next = histRef.current.slice()
    for (let i = 0; i < scrollMul; i++) {
      next.push(frameBands)
    }
    if (next.length > maxHistory) next = next.slice(next.length - maxHistory)
    histRef.current = next
  }, [frameBands])
  function heatColor(v: number) {
    // 强化对比度但保持原色系：
    // 低 -> #24060d, 中(≈#eb1a50) -> 高 -> #ffd6e1
    const clamp = (x: number) => Math.max(0, Math.min(1, x))
    const enhance = (x: number, c = 1.45, pivot = 0.55) => clamp((x - pivot) * c + pivot)
    const t = enhance(clamp(v))
    const mid = 0.6
    const lerp = (a: number, b: number, t: number) => Math.round(a + (b - a) * t)
    if (t <= mid) {
      const u = t / mid
      const r = lerp(36, 235, u) // 24 -> eb
      const g = lerp(6, 26, u)   // 06 -> 1a
      const b = lerp(13, 80, u)  // 0d -> 50
      return `rgb(${r},${g},${b})`
    } else {
      const u = (t - mid) / (1 - mid)
      const r = lerp(235, 255, u)
      const g = lerp(26, 214, u)
      const b = lerp(80, 225, u)
      return `rgb(${r},${g},${b})`
    }
  }
  function renderWaterfall() {
    const cols = Math.min(histRef.current.length, visibleCols)
    const startX = wfOffsetX + (wfW - gap - cols * (cellW + gap))
    const elems: JSX.Element[] = []
    for (let x = 0; x < cols; x++) {
      const idx = histRef.current.length - cols + x
      const bandsVals = histRef.current[idx]
      for (let b = 0; b < bands; b++) {
        const v = bandsVals ? bandsVals[b] || 0 : 0
        const cx = startX + gap + x * (cellW + gap)
        const cy = wfOffsetY + gap + (bands - 1 - b) * (cellW + gap)
        elems.push(<rect key={`${x}-${b}`} x={cx} y={cy} width={cellW} height={cellW} fill={heatColor(v)} opacity={1} />)
      }
    }
    return <>{elems}</>
  }

  const barLen = Math.round(rmsSmoothedRef.current * (w - 60))

  return (
    <div style={{width:w, display:'flex', flexDirection:'column', gap:6, padding:'0 5px'}}>
      <svg width={w-10} height={h} style={{background:bg, border:'1px solid #1d2a3a', borderRadius:6, cursor:'pointer'}} onClick={onToggle}>
        {mode === 'wave' ? renderWave() : mode === 'bars' ? renderBars() : renderWaterfall()}
      </svg>
      {!hideRms && (
        <div title="RMS（均方根）是音频能量/响度的近似，越大代表越响" style={{display:'flex', alignItems:'center', gap:8}}>
          <div style={{width:Math.max(60, w-70), height:8, background:'#3a0b17', borderRadius:4, overflow:'hidden'}}>
            <div style={{width:barLen, height:'100%', background:accent, transition:'width 120ms'}} />
          </div>
          <span style={{fontSize:12, color:'#6b829e'}}>
            RMS {Math.round(rmsSmoothedRef.current*100)}%
          </span>
        </div>
      )}
    </div>
  )
}
