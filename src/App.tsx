import React, { useEffect, useMemo, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
// 使用内置 invoke 写入（后端落盘到桌面），避免引入插件

type DisplayBpm = { bpm: number, confidence: number, state: 'tracking'|'uncertain'|'analyzing', level: number }
type BpmDebug = {
  t_ms: number
  phase: 'estimate'|'silent'|'none'
  level: number
  state: DisplayBpm['state']
  tracking: boolean
  ever_locked: boolean
  hi_cnt: number
  lo_cnt: number
  none_cnt: number
  anchor_bpm?: number | null
  sample_rate: number
  hop_len: number
  raw_bpm?: number | null
  raw_confidence?: number | null
  raw_rms?: number | null
  from_short?: boolean | null
  win_sec?: number | null
  disp_bpm?: number | null
  smoothed_bpm?: number | null
  corr?: 'raw'|'half'|'dbl' | null
}
type BackendLog = { t_ms: number, msg: string }
type AudioViz = { samples: number[], rms: number }

export default function App() {
  const [bpm, setBpm] = useState<number | null>(null)
  const [conf, setConf] = useState<number | null>(null)
  const [state, setState] = useState<DisplayBpm['state']>('analyzing')
  const [level, setLevel] = useState<number>(0)
  const [dbgOpen] = useState<boolean>(false)
  const [dbg, setDbg] = useState<BpmDebug[]>([])
  const [logs, setLogs] = useState<BackendLog[]>([])
  const [updates, setUpdates] = useState<DisplayBpm[]>([])
  const [alwaysOnTop, setAlwaysOnTop] = useState<boolean>(false)
  const [viz, setViz] = useState<AudioViz | null>(null)
  const [vizMode, setVizMode] = useState<'wave'|'bars'|'radial'>('wave')

  useEffect(() => {
    let removeListener: (() => void) | null = null
    ;(async () => {
      try {
        await invoke('start_capture')
        const unlistenA = await listen<DisplayBpm>('bpm_update', (e) => {
          const res = e.payload
          setConf(res.confidence)
          setState(res.state)
          setLevel(res.level)
          // 后端已做过滤：收到即显示；为0则保留上一次
          if (res.bpm > 0) setBpm(res.bpm)
          setUpdates(prev => {
            const max = 1000
            if (prev.length >= max) return [...prev.slice(prev.length - (max - 100)), res]
            return [...prev, res]
          })
        })
        const unlistenB = await listen<BpmDebug>('bpm_debug', (e) => {
          const payload = e.payload as any as BpmDebug
          setDbg((prev) => {
            const max = 1000
            if (prev.length >= max) return [...prev.slice(prev.length - (max - 100)), payload]
            return [...prev, payload]
          })
        })
        const unlistenC = await listen<BackendLog>('bpm_log', (e) => {
          const payload = e.payload as any as BackendLog
          setLogs((prev) => {
            const max = 2000
            if (prev.length >= max) return [...prev.slice(prev.length - (max - 200)), payload]
            return [...prev, payload]
          })
        })
        const unlistenD = await listen<AudioViz>('viz_update', (e) => {
          setViz(e.payload as any as AudioViz)
        })
        removeListener = () => { unlistenA(); unlistenB(); unlistenC(); unlistenD() }
      } catch (err) { console.error('[BOOT] error', err) }
    })()

    return () => { if (removeListener) removeListener() }
  }, [])

  const label = state === 'tracking' ? '稳定' : state === 'analyzing' ? '分析中' : '节拍不稳定'
  const dim = state === 'tracking' ? 1 : 0.5
  const confLabel = conf == null ? '—' : conf >= 0.75 ? '稳定' : conf >= 0.5 ? '较稳' : '不稳'

  // 已固定后端为基础模式，无切换

  const lastDbg = useMemo(() => dbg.length ? dbg[dbg.length - 1] : null, [dbg])
  function clearDebug() { setDbg([]) }

  async function toggleAlwaysOnTop() {
    try {
      const next = !alwaysOnTop
      await invoke('set_always_on_top', { onTop: next })
      setAlwaysOnTop(next)
    } catch (e) {
      console.error('置顶切换失败', e)
    }
  }

  return (
    <main style={{height:'100vh',display:'flex',flexDirection:'column',alignItems:'center',justifyContent:'center',gap:16,background:'#0b0f14',color:'#e6f1ff'}}>
      <h1 style={{margin:0,color:'#8aa4c1',fontSize:18}}>BPM</h1>
      <div style={{fontSize:96,fontWeight:700,letterSpacing:2,opacity:dim}}>{bpm == null ? '—' : Math.round(bpm)}</div>
      <div style={{fontSize:14,color:'#6b829e'}}>{label} · 置信度：{confLabel}</div>
      <div style={{width:240,height:8,background:'#1a2633',borderRadius:4,overflow:'hidden'}}>
        <div style={{height:'100%',width:`${Math.round((level||0)*100)}%`,background:'#2ecc71',transition:'width 120ms'}} />
      </div>

      {/* 简易波形可视化 */}
      <VizPanel viz={viz} mode={vizMode} onToggle={() => setVizMode(m => m==='wave'?'bars':m==='bars'?'radial':'wave')} />

      <div style={{position:'fixed',right:12,top:12,display:'flex',gap:8}}>
        <button onClick={toggleAlwaysOnTop} style={{background: alwaysOnTop ? '#2f4f1f' : '#12202f',color:'#8aa4c1',border:'1px solid #243447',borderRadius:6,padding:'6px 10px',cursor:'pointer'}}>
          {alwaysOnTop ? '已置顶' : '置顶'}
        </button>
      </div>

      {/* 调试面板已移除，仅保留导出功能 */}
    </main>
  )
}

function sampleRateLabel(sr: number) {
  // hop_len 是后端以样本数定义，这里估算 0.5s（见后端），以稳定显示
  // 精确换算用 hop_len / sr
  return sr || 48000
}

function VizPanel({ viz, mode, onToggle }: { viz: AudioViz | null, mode: 'wave'|'bars'|'radial', onToggle: () => void }) {
  const h = 120
  const w = 360
  const bg = '#0f1621'
  const grid = '#152234'
  const line = '#4aa3ff'
  const accent = '#2ecc71'
  const rmsRaw = viz?.rms ?? 0
  const samples = viz?.samples ?? []
  const silentCut = 0.015
  const isSilent = rmsRaw < silentCut

  // 帧间平滑：对样本进行逐点 EMA，减少闪烁（不改变数据频率）
  const [lastSmoothed, setLastSmoothed] = React.useState<number[] | null>(null)
  const smoothSamples = React.useMemo(() => {
    const alpha = 0.35 // 越小越平滑
    const base = isSilent ? new Array(samples.length).fill(0) : samples
    if (!base.length) return [] as number[]
    if (!lastSmoothed || lastSmoothed.length !== base.length) {
      return base
    }
    const out = new Array(base.length)
    for (let i = 0; i < base.length; i++) {
      out[i] = lastSmoothed[i] * (1 - alpha) + base[i] * alpha
    }
    return out
  }, [samples, isSilent, lastSmoothed])
  React.useEffect(() => {
    if (isSilent) { setLastSmoothed(null); return }
    if (smoothSamples.length) setLastSmoothed(smoothSamples)
  }, [smoothSamples, isSilent])

  // RMS 平滑，避免进度条抖动
  const [rmsSmoothed, setRmsSmoothed] = React.useState(0)
  React.useEffect(() => { setRmsSmoothed(v => v * 0.85 + rmsRaw * 0.15) }, [rmsRaw])

  // 自适应增益：对本帧峰值做 EMA 平滑，动态放大视觉振幅（温和版）
  const [peak, setPeak] = React.useState(0.3)
  React.useEffect(() => {
    if (!smoothSamples.length) return
    let localPeak = 0
    for (let i = 0; i < smoothSamples.length; i++) {
      const a = Math.abs(smoothSamples[i] || 0)
      if (a > localPeak) localPeak = a
    }
    // 放慢响应，避免瞬时放大过头
    setPeak(p => isSilent ? 0.2 : (p * 0.95 + localPeak * 0.05))
  }, [smoothSamples, isSilent])
  // 降低基础增益，并限制上下限，取折中视觉效果
  const base = Math.max(0.12, Math.min(0.6, peak))
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

  function renderRadial() {
    // 把样本映射为极坐标花瓣
    const cx = Math.floor(w / 2)
    const cy = Math.floor(h / 2)
    const radius = Math.min(cx, cy) - 4
    const petals = 64
    const step = Math.max(1, Math.floor(smoothSamples.length / petals))
    const elems: JSX.Element[] = []
    for (let p = 0; p < petals; p++) {
      const i0 = p * step
      const i1 = Math.min(smoothSamples.length, i0 + step)
      let acc = 0, cnt = 0
      for (let i = i0; i < i1; i++) { acc += Math.abs(smoothSamples[i] || 0); cnt++ }
      const v = cnt ? acc / cnt : 0
      const vv = Math.min(1, v * gain)
      const theta = (p / petals) * Math.PI * 2
      const r = radius * (0.35 + 0.65 * vv)
      const x = cx + Math.cos(theta) * r
      const y = cy + Math.sin(theta) * r
      elems.push(<line key={p} x1={cx} y1={cy} x2={x} y2={y} stroke={line} strokeWidth={2} strokeLinecap="round" opacity={0.9} />)
    }
    return <>
      <circle cx={cx} cy={cy} r={radius} stroke={grid} strokeWidth={1} fill="none" />
      {elems}
    </>
  }

  const barLen = Math.round(rmsSmoothed * (w - 60))

  return (
    <div style={{width:w, display:'flex', flexDirection:'column', gap:6}}>
      <svg width={w} height={h} style={{background:bg, border:'1px solid #1d2a3a', borderRadius:6, cursor:'pointer'}} onClick={onToggle}>
        {mode === 'wave' ? renderWave() : mode === 'bars' ? renderBars() : renderRadial()}
      </svg>
      <div style={{display:'flex', alignItems:'center', gap:8}}>
        <div style={{width:w-60, height:8, background:'#12202f', borderRadius:4, overflow:'hidden'}}>
          <div style={{width:barLen, height:'100%', background:accent, transition:'width 120ms'}} />
        </div>
        <span style={{fontSize:12, color:'#6b829e'}} title="RMS（均方根）是音频能量/响度的近似，越大代表越响">RMS {Math.round(rmsSmoothed*100)}%</span>
      </div>
    </div>
  )
}
