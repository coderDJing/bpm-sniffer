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
  const elems: JSX.Element[] = []
  // 使用亚像素宽度，确保横向铺满，不再居中留白
  const barW = Math.max(2, Math.min(8, (width - (bars - 1) * gap) / bars))
  const n = Math.max(0, samples.length)
  // 当 bars 小于等于样本数：做分箱平均；当 bars 大于样本数：对样本序列进行线性插值（超采样）
  if (n > 0) {
    if (bars <= n) {
      const binSize = n / bars
      for (let b = 0; b < bars; b++) {
        const s0 = b * binSize
        const s1 = (b + 1) * binSize
        const iStart = Math.max(0, Math.floor(s0))
        const iEnd = Math.min(n, Math.ceil(s1))
        let acc = 0, cnt = 0
        for (let i = iStart; i < iEnd; i++) { acc += Math.abs(samples[i] || 0); cnt++ }
        const v = cnt ? acc / cnt : 0
        const vv = Math.min(1, v * gain)
        const x = b * (barW + gap)
        const bh = vv * (height - 4)
        const y = height - 2 - bh
        elems.push(<rect key={b} x={x} y={y} width={barW} height={bh} fill={barColor} opacity={0.85} />)
      }
    } else {
      const denom = Math.max(1, bars - 1)
      for (let b = 0; b < bars; b++) {
        const u = b / denom
        const s = u * (n - 1)
        const i0 = Math.floor(s)
        const i1 = Math.min(n - 1, i0 + 1)
        const t = s - i0
        const v0 = samples[i0] || 0
        const v1 = samples[i1] || 0
        const v = Math.abs(v0 * (1 - t) + v1 * t)
        const vv = Math.min(1, v * gain)
        const x = b * (barW + gap)
        const bh = vv * (height - 4)
        const y = height - 2 - bh
        elems.push(<rect key={b} x={x} y={y} width={barW} height={bh} fill={barColor} opacity={0.85} />)
      }
    }
  }
  return <>{elems}</>
}


