import React from 'react'

type WaterfallVizProps = {
  width: number
  height: number
  bands: number
  gap: number
  cell: number
  cellX?: number
  history: number[][]
  heatColor: (v: number) => string
  overrideOffsetY?: number
}

export default function WaterfallViz({ width, height, bands, gap, cell, cellX, history, heatColor, overrideOffsetY }: WaterfallVizProps) {
  // 纵向由 cell 控制；横向使用固定单元宽度 cellX（默认等于 cell）来决定可见列数
  const cx = Math.max(1, Math.floor((cellX ?? cell)))
  const visibleCols = Math.max(1, Math.floor((width - gap) / (cx + gap)))
  const cols = visibleCols
  const wfH = Math.min(height, bands * (cell + gap) + gap)
  const wfOffsetY = overrideOffsetY != null ? overrideOffsetY : Math.max(0, Math.floor((height - wfH) / 2))
  // 居中绘制，左右保持对称 gap
  const totalW = gap + cols * (cx + gap)
  const startX = Math.max(0, Math.floor((width - totalW) / 2))
  const elems: JSX.Element[] = []
  for (let x = 0; x < cols; x++) {
    const idx = history.length - cols + x
    const bandsVals = (idx >= 0 && idx < history.length) ? history[idx] : null
    for (let b = 0; b < bands; b++) {
      const v = bandsVals ? bandsVals[b] || 0 : 0
      const px = startX + gap + x * (cx + gap)
      const cy = wfOffsetY + gap + (bands - 1 - b) * (cell + gap)
      elems.push(<rect key={`${x}-${b}`} x={px} y={cy} width={cx} height={cell} fill={heatColor(v)} opacity={1} />)
    }
  }
  return <>{elems}</>
}


