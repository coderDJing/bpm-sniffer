import React from 'react'

type WaterfallVizProps = {
  width: number
  height: number
  bands: number
  gap: number
  cell: number
  history: number[][]
  heatColor: (v: number) => string
  overrideOffsetY?: number
}

export default function WaterfallViz({ width, height, bands, gap, cell, history, heatColor, overrideOffsetY }: WaterfallVizProps) {
  const visibleCols = Math.max(1, Math.floor((width - gap) / (cell + gap)))
  const wfW = Math.min(width, visibleCols * (cell + gap) + gap)
  const wfH = Math.min(height, bands * (cell + gap) + gap)
  const wfOffsetX = Math.max(0, Math.floor((width - wfW) / 2))
  const wfOffsetY = overrideOffsetY != null ? overrideOffsetY : Math.max(0, Math.floor((height - wfH) / 2))

  const cols = Math.min(history.length, visibleCols)
  const startX = wfOffsetX + (wfW - gap - cols * (cell + gap))
  const elems: JSX.Element[] = []
  for (let x = 0; x < cols; x++) {
    const idx = history.length - cols + x
    const bandsVals = history[idx]
    for (let b = 0; b < bands; b++) {
      const v = bandsVals ? bandsVals[b] || 0 : 0
      const cx = startX + gap + x * (cell + gap)
      const cy = wfOffsetY + gap + (bands - 1 - b) * (cell + gap)
      elems.push(<rect key={`${x}-${b}`} x={cx} y={cy} width={cell} height={cell} fill={heatColor(v)} opacity={1} />)
    }
  }
  return <>{elems}</>
}


