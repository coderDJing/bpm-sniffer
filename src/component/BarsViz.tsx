import React from 'react'

type BarsVizProps = {
  width: number
  height: number
  samples: number[]
  gain: number
  barColor: string
  bars?: number
}

export default function BarsViz({ width, height, samples, gain, barColor, bars = 64 }: BarsVizProps) {
  // 将 256 点折叠为 bars 根柱，每根取局部绝对值均值
  const gap = 1
  const step = Math.max(1, Math.floor(samples.length / bars))
  const elems: JSX.Element[] = []
  const barW = Math.max(2, Math.floor((width - (bars - 1) * gap) / bars))
  const totalW = barW * bars + gap * (bars - 1)
  const offsetX = Math.max(0, Math.floor((width - totalW) / 2))
  for (let b = 0; b < bars; b++) {
    const i0 = b * step
    const i1 = Math.min(samples.length, i0 + step)
    let acc = 0, cnt = 0
    for (let i = i0; i < i1; i++) { acc += Math.abs(samples[i] || 0); cnt++ }
    const v = cnt ? acc / cnt : 0
    const vv = Math.min(1, v * gain)
    const x = offsetX + b * (barW + gap)
    const bh = Math.round(vv * (height - 4))
    const y = height - 2 - bh
    elems.push(<rect key={b} x={x} y={y} width={barW} height={bh} fill={barColor} opacity={0.85} />)
  }
  return <>{elems}</>
}


