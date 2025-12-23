import React from 'react'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { AudioViz, DisplayKey } from '../types'

type FloatBallProps = {
  themeName: 'dark'|'light'
  bpm: number
  conf: number | null
  viz: AudioViz | null
  keyMode: 'note'|'camelot'
  keyNote: string | null
  keyCamelot: string | null
  keyConf: number | null
  keyState: DisplayKey['state']
  showWaiting: boolean
  onExit: () => Promise<void>
  isLockedHighlight: boolean
}

export default function FloatBall({ themeName, bpm, conf, viz, keyMode, keyNote, keyCamelot, keyConf, keyState, showWaiting, onExit, isLockedHighlight }: FloatBallProps) {
  const darkTheme = {
    background: 'rgba(20,6,10,0.82)',
    textPrimary: '#ffffff',
    ring: '#eb1a50',
    confGray: '#9aa3ab'
  }
  const lightTheme = {
    background: 'rgba(255,244,247,0.82)',
    textPrimary: '#1b0a10',
    ring: '#eb1a50',
    confGray: '#8a8f96'
  }
  const theme = themeName === 'dark' ? darkTheme : lightTheme
  const lastClickRef = React.useRef<number>(0)
  const dragStartRef = React.useRef<{x:number,y:number}|null>(null)
  const accent = theme.ring || '#eb1a50'
  const ballSize = 58
  const baseStroke = 1.68
  const widthGain = 2.24
  const radiusGain = 2.56
  const innerRadiusGain = 0.96
  const shadowBlur = 9.6
  const segments = 64
  const segmentGap = 0.12
  const marginPx = Math.ceil(shadowBlur + (baseStroke + widthGain) / 2 + radiusGain + 2)
  const canvasSize = ballSize + marginPx * 2

  const toRgba = React.useCallback(
    (alpha: number) => {
      const hex = accent.startsWith('#') ? accent.slice(1) : null
      if (!hex || (hex.length !== 3 && hex.length !== 6)) {
        return `rgba(235,26,80,${alpha})`
      }
      const full = hex.length === 3 ? hex.split('').map((c) => c + c).join('') : hex
      const num = parseInt(full, 16)
      const r = (num >> 16) & 255
      const g = (num >> 8) & 255
      const b = num & 255
      return `rgba(${r},${g},${b},${alpha})`
    },
    [accent]
  )

  const canvasRef = React.useRef<HTMLCanvasElement | null>(null)
  // rAF ?????? viz ?????????????????????????????
  const lastVizRef = React.useRef<AudioViz | null>(null)
  React.useEffect(() => { lastVizRef.current = viz }, [viz])
  React.useEffect(() => {
    let rafId = 0
    const tick = () => {
      const cvs = canvasRef.current
      if (cvs) {
        const ctx = cvs.getContext('2d')
        if (ctx) {
          const dpr = Math.max(1, Math.floor(window.devicePixelRatio || 1))
          const cssW = canvasSize
          const cssH = canvasSize
          if (cvs.width !== cssW * dpr || cvs.height !== cssH * dpr) {
            cvs.width = cssW * dpr; cvs.height = cssH * dpr
          }
          cvs.style.width = cssW + 'px'; cvs.style.height = cssH + 'px'
          ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
          ctx.clearRect(0,0,cssW,cssH)
          const cx = cssW / 2
          const cy = cssH / 2
          const samples = lastVizRef.current?.samples ?? []
          const rms = lastVizRef.current?.rms ?? 0
          const silent = rms <= 0.002
          const energyBoost = Math.min(2.2, 0.85 + rms * 2.4)
          const rBase = Math.max(8, ballSize / 2 - baseStroke / 2 + radiusGain)
          ctx.lineCap = 'round' as CanvasLineCap
          ctx.lineJoin = 'round' as CanvasLineJoin

          ctx.save()
          ctx.shadowBlur = 0
          ctx.lineWidth = Math.max(1.1, baseStroke * (silent ? 0.85 : 0.7))
          ctx.strokeStyle = toRgba(silent ? 0.5 : 0.25)
          ctx.beginPath(); ctx.arc(cx, cy, rBase, 0, Math.PI * 2, false); ctx.stroke()
          ctx.restore()

          const seg = (Math.PI * 2) / segments
          for (let i = 0; i < segments; i++) {
            let acc = 0
            let cnt = 0
            if (samples.length) {
              const i0 = Math.floor((i / segments) * samples.length)
              const i1 = Math.floor(((i + 1) / segments) * samples.length)
              for (let j = i0; j < i1; j++) { acc += Math.abs(samples[j] || 0); cnt++ }
            }
            const avg = cnt ? acc / cnt : 0
            const amp = Math.pow(Math.min(1, avg * energyBoost * 1.8), 0.6)
            const alpha = Math.min(1, (silent ? 0.32 : 0.2) + amp * (silent ? 0.55 : 0.75))
            const a0 = -Math.PI / 2 + i * seg + seg * segmentGap * 0.5
            const a1 = a0 + seg * (1 - segmentGap)
            const outerR = rBase + amp * radiusGain
            ctx.lineWidth = baseStroke + amp * widthGain
            ctx.strokeStyle = toRgba(alpha)
            ctx.beginPath(); ctx.arc(cx, cy, outerR, a0, a1, false); ctx.stroke()

            const innerR = Math.max(4, rBase - amp * innerRadiusGain)
            const innerAlpha = Math.min(1, (silent ? 0.25 : 0.16) + amp * 0.55)
            ctx.lineWidth = 1 + amp * 1.1
            ctx.strokeStyle = toRgba(innerAlpha)
            ctx.beginPath(); ctx.arc(cx, cy, innerR, a0, a1, false); ctx.stroke()
          }
          ctx.shadowBlur = 0
        }
      }
      rafId = requestAnimationFrame(tick)
    }
    rafId = requestAnimationFrame(tick)
    return () => { if (rafId) cancelAnimationFrame(rafId) }
  }, [themeName, ballSize, canvasSize, toRgba, segments, segmentGap, baseStroke, radiusGain, widthGain, innerRadiusGain, shadowBlur])


  async function handlePointerDown(e: React.PointerEvent) {
    dragStartRef.current = { x: e.clientX, y: e.clientY }
    const onMove = async (ev: PointerEvent) => {
      const s = dragStartRef.current
      if (!s) return
      const dx = Math.abs(ev.clientX - s.x)
      const dy = Math.abs(ev.clientY - s.y)
      if (dx > 3 || dy > 3) {
        dragStartRef.current = null
        try {
          const win = getCurrentWebviewWindow()
          // 双保险：先尝试系统拖动；若失败则手动移动到光标附近（需要 window 权限）
          try { await win.startDragging() } catch (e) {
            try {
              const x = Math.max(0, ev.screenX - Math.floor(ballSize/2))
              const y = Math.max(0, ev.screenY - Math.floor(ballSize/2))
              // @ts-ignore 支持 Tauri v2 setPosition(Position)
              await (win as any).setPosition({ x, y })
              try { await (window as any).__TAURI_INVOKE__('save_float_pos', { x, y }) } catch {}
            } catch (e2) { }
          }
        } catch {}
        window.removeEventListener('pointermove', onMove)
        window.removeEventListener('pointerup', onUp)
      }
    }
    const onUp = async (_ev: PointerEvent) => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      if (dragStartRef.current) {
        // 未触发拖动 -> 认为是点击
        dragStartRef.current = null
        const now = Date.now()
        if (now - (lastClickRef.current || 0) < 300) {
          lastClickRef.current = 0
          await onExit()
          return
        }
        lastClickRef.current = now
        // 悬浮球不再支持单击刷新，保持静默
      }
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }

  const confGray = theme.confGray
  const color = isLockedHighlight ? theme.textPrimary : (conf == null ? confGray : (conf >= 0.5 ? theme.textPrimary : confGray))
  const bpmFontPx = 20
  const keyFontPx = 16
  const rootStyle: React.CSSProperties = {height:'100vh',display:'flex',alignItems:'center',justifyContent:'center',background:'transparent', cursor:'default'}
  const keyText = showWaiting ? '-' : ((keyMode === 'camelot' ? keyCamelot : keyNote) || '-')
  const keyStable = !showWaiting && keyState === 'tracking' && keyConf != null && keyConf >= 0.55 && keyText !== '-'
  const keyColor = keyStable ? theme.textPrimary : confGray
  const textStyle: React.CSSProperties = {fontSize:bpmFontPx,fontWeight:700,color,letterSpacing:1,lineHeight:bpmFontPx + 'px'}
  const keyStyle: React.CSSProperties = {fontSize:keyFontPx,fontWeight:700,color:keyColor,letterSpacing:1,lineHeight:keyFontPx + 'px'}
  return (
    <main style={rootStyle}>
      <div
        onPointerDown={handlePointerDown}
        onDoubleClick={async (e) => { e.preventDefault(); e.stopPropagation(); await onExit() }}
        style={{
          width: canvasSize,
          height: canvasSize + 32,
          position: 'relative',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          gap: 10,
          cursor: 'default',
          background: 'transparent'
        }}
      >
        <div style={{ position: 'relative', width: canvasSize, height: canvasSize, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          <canvas
            ref={canvasRef}
            width={canvasSize}
            height={canvasSize}
            style={{ position: 'absolute', inset: 0, pointerEvents: 'none', zIndex: 2, filter: 'none' }}
          />
          <div
            style={{
              width: ballSize,
              height: ballSize,
              borderRadius: ballSize / 2,
              background: theme.background,
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 2,
              position: 'relative',
              zIndex: 3,
              boxShadow: 'none'
            }}
          >
            <div style={textStyle}>{Math.round(bpm || 0)}</div>
            <div style={keyStyle}>{keyText}</div>
          </div>
        </div>
      </div>
      <style>{`@keyframes spin360{from{transform:rotate(0)}to{transform:rotate(360deg)}}`}</style>
    </main>
  )
}
