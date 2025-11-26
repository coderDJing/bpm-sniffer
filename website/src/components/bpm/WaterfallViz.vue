<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  width: number
  height: number
  bands: number
  gap: number
  cell: number
  cellX?: number
  history: number[][]
  heatColor: (v: number) => string
  overrideOffsetY?: number
}>()

const cx = computed(() => Math.max(1, Math.floor(props.cellX ?? props.cell)))

const cols = computed(() => Math.max(1, Math.floor((props.width - props.gap) / (cx.value + props.gap))))

const wfH = computed(() => Math.min(props.height, props.bands * (props.cell + props.gap) + props.gap))

const wfOffsetY = computed(() => {
  if (props.overrideOffsetY != null) return props.overrideOffsetY
  return Math.max(0, Math.floor((props.height - wfH.value) / 2))
})

const totalW = computed(() => props.gap + cols.value * (cx.value + props.gap))
const startX = computed(() => Math.max(0, Math.floor((props.width - totalW.value) / 2)))

const cells = computed(() => {
  const out: { key: string; x: number; y: number; color: string }[] = []
  for (let x = 0; x < cols.value; x++) {
    const idx = props.history.length - cols.value + x
    const bandsVals = idx >= 0 && idx < props.history.length ? props.history[idx] : null
    for (let b = 0; b < props.bands; b++) {
      const v = bandsVals ? bandsVals[b] || 0 : 0
      const px = startX.value + props.gap + x * (cx.value + props.gap)
      const cy = wfOffsetY.value + props.gap + (props.bands - 1 - b) * (props.cell + props.gap)
      out.push({
        key: `${x}-${b}`,
        x: px,
        y: cy,
        color: props.heatColor(v)
      })
    }
  }
  return out
})
</script>

<template>
  <rect
    v-for="item in cells"
    :key="item.key"
    :x="item.x"
    :y="item.y"
    :width="cx"
    :height="cell"
    :fill="item.color"
    opacity="1"
  />
</template>
