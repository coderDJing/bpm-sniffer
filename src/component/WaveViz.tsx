import React from 'react'

type WaveVizProps = {
  width: number
  height: number
  samples: number[]
  gain: number
  gridColor: string
  lineColor: string
}

export default function WaveViz({ width, height, samples, gain, gridColor, lineColor }: WaveVizProps) {
  const mid = Math.floor(height / 2)
  const clamped = (v: number) => Math.max(-1, Math.min(1, v))
  const path = React.useMemo(() => {
    if (!samples.length || width <= 1) return ''
    const dpr = (typeof window !== 'undefined') ? Math.max(1, (window as any).devicePixelRatio || 1) : 1
    const targetPoints = Math.max(2, Math.floor(width * dpr))
    const srcLen = samples.length
    const segs: string[] = []

    // 当源点数远大于像素时，做分箱平均；否则做线性插值，保证不同宽度下一致的视觉密度
    const useBinning = srcLen >= targetPoints
    if (useBinning) {
      const binSize = srcLen / (targetPoints - 1)
      for (let xi = 0; xi < targetPoints; xi++) {
        const start = Math.floor(Math.max(0, Math.min(srcLen - 1, xi * binSize)))
        const end = Math.floor(Math.max(0, Math.min(srcLen, (xi + 1) * binSize)))
        let acc = 0, cnt = 0
        for (let i = start; i < Math.max(start + 1, end); i++) { acc += samples[i] || 0; cnt++ }
        const v = cnt ? acc / cnt : 0
        const x = xi / dpr
        const y = mid - (clamped(v) * gain * (height * 0.40))
        segs.push(`${xi === 0 ? 'M' : 'L'}${x},${y}`)
      }
    } else {
      for (let xi = 0; xi < targetPoints; xi++) {
        const u = xi / (targetPoints - 1)
        const s = u * (srcLen - 1)
        const i0 = Math.floor(s)
        const i1 = Math.min(srcLen - 1, i0 + 1)
        const t = s - i0
        const v0 = samples[i0] || 0
        const v1 = samples[i1] || 0
        const v = v0 * (1 - t) + v1 * t
        const x = xi / dpr
        const y = mid - (clamped(v) * gain * (height * 0.40))
        segs.push(`${xi === 0 ? 'M' : 'L'}${x},${y}`)
      }
    }
    return segs.join(' ')
  }, [samples, gain, width, height, mid])

  return <>
    <line x1={0} y1={mid} x2={width} y2={mid} stroke={gridColor} strokeWidth={1} />
    {path && <path d={path} stroke={lineColor} strokeWidth={1.5} fill="none" />}
  </>
}


