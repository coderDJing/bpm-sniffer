import React from 'react'
import WaveViz from './WaveViz'
import BarsViz from './BarsViz'
import WaterfallViz from './WaterfallViz'
import fullScreenBlack from '../assets/fullScreenBlack.png'
import fullScreenWhite from '../assets/fullScreenWhite.png'
import { t, tn } from '../i18n'
import { AudioViz } from '../types'

type VizPanelProps = {
  theme: any
  hideRms: boolean
  viz: AudioViz | null
  mode: 'wave'|'bars'|'waterfall'
  onToggle: () => void
  themeName: 'dark'|'light'
}

export default function VizPanel({ theme, hideRms, viz, mode, onToggle, themeName }: VizPanelProps) {
  // 自适应高度：在默认窗口高度（≈390）时保持 120px，随着窗口拉高按比例增大，设上下限
  const baseWindowH = 390
  const baseVizH = 120
  const vh = typeof window !== 'undefined' ? window.innerHeight : baseWindowH
  const h = Math.max(100, Math.min(300, Math.floor(baseVizH + Math.max(0, vh - baseWindowH) * 0.7)))
  const w = Math.max(180, Math.floor(window.innerWidth))
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

  // Waterfall 历史与参数占位（自适应将在计算有效高度后进行）
  const histRef = React.useRef<number[][]>([])
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
  // barLen 延后基于 svgW 计算
  // 全屏图标显隐与全屏控制
  const containerRef = React.useRef<HTMLDivElement | null>(null)
  const [hovered, setHovered] = React.useState(false)
  const [isFullscreen, setIsFullscreen] = React.useState(false)
  React.useEffect(() => {
    function onFsChange() {
      setIsFullscreen(!!document.fullscreenElement)
    }
    document.addEventListener('fullscreenchange', onFsChange)
    return () => document.removeEventListener('fullscreenchange', onFsChange)
  }, [])
  async function enterFullscreen(e?: React.MouseEvent) {
    if (e) { e.stopPropagation(); e.preventDefault() }
    const el = containerRef.current
    if (!el) return
    try {
      if (el.requestFullscreen) await el.requestFullscreen()
    } catch {}
  }
  const fsIcon = themeName === 'dark' ? fullScreenWhite : fullScreenBlack

  // 基于全屏状态决定可视化高度
  const fullH = typeof window !== 'undefined'
    ? Math.max(window.innerHeight || 0, (window.screen && (window.screen as any).height) || 0, document.documentElement?.clientHeight || 0)
    : baseWindowH
  const effectiveH = isFullscreen
    ? Math.max(100, Math.floor(fullH - (hideRms ? 20 : 36)))
    : h
  // 依据有效高度自适应 Waterfall 的 bands 与 cell，并重新计算垂直偏移
  const gap = 1
  const baseH = baseVizH
  const baseBands = 16
  // 高度越大频带越多：从 16 线性增加到 24（最长到 32），避免过密
  const scaleH = Math.max(0, Math.min(1, (effectiveH - 120) / 360))
  const bands = Math.max(8, Math.min(32, Math.round(baseBands + scaleH * 8)))
  const baseCell = 4
  const baseContentH = Math.min(baseH, bands * (baseCell + gap) + gap)
  const idealPaddingY = Math.max(0, Math.floor((baseH - baseContentH) / 2))
  const targetContentH2 = Math.max(10, effectiveH - idealPaddingY * 2)
  const cell2 = Math.max(2, Math.min(16, Math.floor((targetContentH2 - gap) / bands - gap)))
  const wfH2 = Math.min(effectiveH, bands * (cell2 + gap) + gap)
  const extraY2 = Math.max(0, Math.floor((effectiveH - 2 * idealPaddingY - wfH2) / 2))
  const wfOffsetYEffective = Math.max(0, idealPaddingY + extraY2)

  // 将一帧样本折叠为自适应 bands 段能量
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
  }, [smoothSamples, gain, bands])

  // 维护历史（右侧为最新），按固定滚动倍速推进；当 bands 变化时不回溯转换历史，仅按新 bands 写入
  const scrollMul = 6
  const maxHistory = 600
  React.useEffect(() => {
    if (!frameBands.length) return
    let next = histRef.current.slice()
    for (let i = 0; i < scrollMul; i++) next.push(frameBands)
    if (next.length > maxHistory) next = next.slice(next.length - maxHistory)
    histRef.current = next
  }, [frameBands])
  const visibleCols2 = Math.max(1, Math.floor((w - gap) / (cell2 + gap)))

  // 按宽度自适应柱状图根数：目标单元宽度（含1px间隙）约 5px，限制区间 [32, 256]
  const targetCellPx = 5
  const barsCount = Math.max(32, Math.min(256, Math.floor(w / targetCellPx)))

  return (
    <div
      ref={containerRef}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{width:w, display:'flex', flexDirection:'column', gap:6, padding:0}}
    >
      <div style={{position:'relative'}}>
        {(() => {
          const paddingX = isFullscreen ? 0 : 5
          const svgW = Math.max(10, w - paddingX * 2)
          return (
            <svg width={svgW} height={effectiveH} style={{background:bg, border:'1px solid #1d2a3a', borderRadius:6, cursor:'pointer', display:'block', margin:'0 auto'}} onClick={onToggle}>
              {mode === 'wave' && (
                <WaveViz width={svgW} height={effectiveH} samples={smoothSamples} gain={gain} gridColor={grid} lineColor={line} />
              )}
              {mode === 'bars' && (() => {
                const barGap = 1
                const minBarW = 2
                const barsWanted = Math.max(32, Math.min(2048, Math.floor((svgW + barGap) / (minBarW + barGap))))
                return <BarsViz width={svgW} height={effectiveH} samples={smoothSamples} gain={gain} barColor={line} bars={barsWanted} />
              })()}
              {mode === 'waterfall' && (
                <WaterfallViz width={svgW} height={effectiveH} bands={bands} gap={gap} cell={cell2} cellX={Math.max(2, Math.min(8, Math.round(cell2)))} history={histRef.current} heatColor={heatColor} overrideOffsetY={wfOffsetYEffective} />
              )}
            </svg>
          )
        })()}
        {/* 悬浮全屏图标 */}
        {!isFullscreen && (
          <button
            onClick={enterFullscreen}
            title={tn('全屏','Fullscreen')}
            style={{
              position:'absolute',
              right:8,
              top:8,
              width:26,
              height:26,
              padding:0,
              margin:0,
              border:'none',
              background:'transparent',
              cursor:'pointer',
              opacity: hovered ? 1 : 0,
              transform: hovered ? 'translateY(0) scale(1)' : 'translateY(-4px) scale(0.96)',
              transition:'opacity 160ms ease, transform 160ms ease'
            }}
          >
            <img src={fsIcon} alt={tn('全屏','Fullscreen')} width={26} height={26} draggable={false} />
          </button>
        )}
      </div>
      {!hideRms && (
        (() => {
          const paddingX = isFullscreen ? 0 : 5
          const svgW = Math.max(10, w - paddingX * 2)
          const trackW = Math.max(60, svgW - 60)
          const fillW = Math.max(0, Math.min(trackW, Math.round(rmsSmoothedRef.current * trackW)))
          return (
            <div title={t('rms_tooltip')} style={{display:'flex', alignItems:'center', gap:8, height:14, width:svgW, margin:'0 auto'}}>
              <div style={{width:trackW, height:8, background:'#3a0b17', borderRadius:4, overflow:'hidden'}}>
                <div style={{width:fillW, height:'100%', background:accent, transition:'width 120ms'}} />
              </div>
              <span style={{fontSize:12, lineHeight:'14px', color:'#6b829e'}}>
                RMS {Math.round(rmsSmoothedRef.current*100)}%
              </span>
            </div>
          )
        })()
      )}
    </div>
  )
}
