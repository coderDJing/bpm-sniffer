<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  width: number
  height: number
  samples: number[]
  gain: number
  barColor: string
  bars?: number
}>()

const gap = 1

const barWidth = computed(() => {
  const bars = props.bars ?? 64
  const width = Math.max(10, props.width)
  return Math.max(2, Math.min(8, (width - (bars - 1) * gap) / bars))
})

const barsData = computed(() => {
  const bars = props.bars ?? 64
  const width = Math.max(10, props.width)
  const height = Math.max(20, props.height)
  const barW = barWidth.value
  const samples = props.samples || []
  const n = Math.max(0, samples.length)
  const items: { x: number; y: number; h: number }[] = []
  if (!n) return items
  if (bars <= n) {
    const binSize = n / bars
    for (let b = 0; b < bars; b++) {
      const s0 = b * binSize
      const s1 = (b + 1) * binSize
      const iStart = Math.max(0, Math.floor(s0))
      const iEnd = Math.min(n, Math.ceil(s1))
      let acc = 0
      let cnt = 0
      for (let i = iStart; i < iEnd; i++) {
        acc += Math.abs(samples[i] || 0)
        cnt++
      }
      const v = cnt ? acc / cnt : 0
      const vv = Math.min(1, v * props.gain)
      const x = b * (barW + gap)
      const bh = vv * (height - 4)
      const y = height - 2 - bh
      items.push({ x, y, h: bh })
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
      const vv = Math.min(1, v * props.gain)
      const x = b * (barW + gap)
      const bh = vv * (height - 4)
      const y = height - 2 - bh
      items.push({ x, y, h: bh })
    }
  }
  return items
})
</script>

<template>
  <rect
    v-for="(bar, index) in barsData"
    :key="index"
    :x="bar.x"
    :y="bar.y"
    :width="barWidth"
    :height="Math.max(1.5, bar.h)"
    :fill="barColor"
    opacity="0.85"
    rx="1.5"
  />
</template>
