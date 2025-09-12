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
    if (!samples.length) return ''
    const n = Math.max(1, samples.length - 1)
    let segs: string[] = []
    for (let i = 0; i < samples.length; i++) {
      const x = Math.round((i / n) * (width - 1))
      const y = mid - Math.round(clamped(samples[i] || 0) * gain * (height * 0.40))
      segs.push(`${i === 0 ? 'M' : 'L'}${x},${y}`)
    }
    return segs.join(' ')
  }, [samples, gain, width, height, mid])

  return <>
    <line x1={0} y1={mid} x2={width} y2={mid} stroke={gridColor} strokeWidth={1} />
    {path && <path d={path} stroke={lineColor} strokeWidth={1.5} fill="none" />}
  </>
}


