<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  width: number
  height: number
  samples: number[]
  gain: number
  gridColor: string
  lineColor: string
}>()

const mid = computed(() => Math.floor(props.height / 2))

const path = computed(() => {
  const samples = props.samples || []
  const width = Math.max(2, props.width)
  const height = props.height
  if (!samples.length || width <= 1) return ''
  const targetPoints = Math.max(2, Math.floor(width))
  const srcLen = samples.length
  const segs: string[] = []
  const useBinning = srcLen >= targetPoints
  if (useBinning) {
    const binSize = srcLen / (targetPoints - 1)
    for (let xi = 0; xi < targetPoints; xi++) {
      const start = Math.floor(Math.max(0, Math.min(srcLen - 1, xi * binSize)))
      const end = Math.floor(Math.max(0, Math.min(srcLen, (xi + 1) * binSize)))
      let acc = 0
      let cnt = 0
      const safeEnd = Math.max(start + 1, end)
      for (let i = start; i < safeEnd; i++) {
        acc += samples[i] || 0
        cnt++
      }
      const v = cnt ? acc / cnt : 0
      const x = xi
      const y = mid.value - (Math.max(-1, Math.min(1, v)) * props.gain * (height * 0.4))
      segs.push(`${xi === 0 ? 'M' : 'L'}${x},${y.toFixed(2)}`)
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
      const x = xi
      const y = mid.value - (Math.max(-1, Math.min(1, v)) * props.gain * (height * 0.4))
      segs.push(`${xi === 0 ? 'M' : 'L'}${x},${y.toFixed(2)}`)
    }
  }
  return segs.join(' ')
})
</script>

<template>
  <line :x1="0" :y1="mid" :x2="width" :y2="mid" :stroke="gridColor" stroke-width="1" />
  <path v-if="path" :d="path" :stroke="lineColor" stroke-width="1.6" fill="none" stroke-linecap="round" />
</template>
