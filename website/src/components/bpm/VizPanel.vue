<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import WaveViz from './WaveViz.vue'
import BarsViz from './BarsViz.vue'
import WaterfallViz from './WaterfallViz.vue'
import fullscreenLight from '../../assets/bpm/fullScreenWhite.png'
import fullscreenDark from '../../assets/bpm/fullScreenBlack.png'

type VizMode = 'wave' | 'bars' | 'waterfall'

type AudioViz = { samples: number[]; rms: number }

const props = withDefaults(
  defineProps<{
    theme: Record<string, string>
    hideRms?: boolean
    viz?: AudioViz | null
    mode?: VizMode
    themeName?: 'dark' | 'light'
    width?: number
    baseHeight?: number
  }>(),
  {
    hideRms: false,
    viz: () => ({ samples: [], rms: 0 }),
    mode: 'wave',
    themeName: 'dark',
    width: 360,
    baseHeight: 140
  }
)

const emit = defineEmits<{ toggle: [] }>()

const containerRef = ref<HTMLElement | null>(null)
const hovered = ref(false)
const isFullscreen = ref(false)

function onFsChange() {
  isFullscreen.value = !!document.fullscreenElement
}

async function enterFullscreen(e?: MouseEvent) {
  e?.stopPropagation()
  e?.preventDefault()
  const el = containerRef.value
  if (!el) return
  try {
    if (el.requestFullscreen) await el.requestFullscreen()
  } catch {
    // ignore
  }
}

onMounted(() => {
  document.addEventListener('fullscreenchange', onFsChange)
})

onUnmounted(() => {
  document.removeEventListener('fullscreenchange', onFsChange)
})

const effectiveWidth = computed(() => {
  if (isFullscreen.value && typeof window !== 'undefined') {
    return Math.max(window.innerWidth || document.documentElement?.clientWidth || 600, 360)
  }
  return props.width
})

const effectiveHeight = computed(() => {
  if (isFullscreen.value && typeof window !== 'undefined') {
    const fullH = Math.max(
      window.innerHeight || 0,
      (window.screen && (window.screen as any).height) || 0,
      document.documentElement?.clientHeight || 0
    )
    return Math.max(140, fullH - (props.hideRms ? 20 : 50))
  }
  return props.baseHeight
})

const smoothSamples = ref<number[]>([])
const prevSamples = ref<number[]>([])
const silentCut = 0.015
const prevRms = ref(0)
const fastSilent = ref(false)
const isSilent = ref(false)

watch(
  () => props.viz?.rms ?? 0,
  (rms) => {
    const drop = prevRms.value > 0.08 && rms < prevRms.value * 0.25
    fastSilent.value = drop
    prevRms.value = rms
    isSilent.value = rms < silentCut || drop
  },
  { immediate: true }
)

watch(
  () => props.viz?.samples,
  (samples = []) => {
    const base = isSilent.value ? new Array(samples.length).fill(0) : samples
    if (!base.length) {
      smoothSamples.value = []
      prevSamples.value = []
      return
    }
    if (!prevSamples.value.length || prevSamples.value.length !== base.length) {
      smoothSamples.value = [...base]
      prevSamples.value = [...base]
      return
    }
    const alpha = 0.35
    const out = new Array(base.length)
    for (let i = 0; i < base.length; i++) {
      out[i] = prevSamples.value[i] * (1 - alpha) + base[i] * alpha
    }
    smoothSamples.value = out
    prevSamples.value = out
  },
  { immediate: true }
)

const rmsSmoothed = ref(0)
watch(
  () => props.viz?.rms ?? 0,
  (rms) => {
    if (isSilent.value) {
      rmsSmoothed.value = 0
      return
    }
    const alphaUp = 0.15
    const alphaDown = 0.35
    const alpha = rms < rmsSmoothed.value ? alphaDown : alphaUp
    rmsSmoothed.value = rmsSmoothed.value * (1 - alpha) + rms * alpha
  },
  { immediate: true }
)

const peak = ref(0.3)
watch(
  () => smoothSamples.value,
  (samples) => {
    if (!samples.length) {
      peak.value = 0.2
      return
    }
    let localPeak = 0
    for (let i = 0; i < samples.length; i++) {
      const a = Math.abs(samples[i] || 0)
      if (a > localPeak) localPeak = a
    }
    peak.value = isSilent.value ? 0.2 : peak.value * 0.95 + localPeak * 0.05
  },
  { immediate: true }
)

const gain = computed(() => {
  const base = Math.max(0.12, Math.min(0.6, peak.value || 0.2))
  let g = 0.6 / base
  return Math.max(0.8, Math.min(2.2, g))
})

const gap = 1
const baseBands = 16
const maxHistory = 600
const hist = ref<number[][]>([])

const bands = computed(() => {
  const scaleH = Math.max(0, Math.min(1, (effectiveHeight.value - 120) / 360))
  return Math.max(8, Math.min(32, Math.round(baseBands + scaleH * 8)))
})

watch(
  () => [smoothSamples.value, bands.value],
  () => {
    const samples = smoothSamples.value
    if (!samples.length) return
    const b = bands.value
    const step = Math.max(1, Math.floor(samples.length / b))
    const frame: number[] = new Array(b).fill(0)
    for (let band = 0; band < b; band++) {
      const i0 = band * step
      const i1 = Math.min(samples.length, i0 + step)
      let acc = 0
      let cnt = 0
      for (let i = i0; i < i1; i++) {
        acc += Math.abs(samples[i] || 0)
        cnt++
      }
      const v = cnt ? acc / cnt : 0
      frame[band] = Math.min(1, v * gain.value)
    }
    const next = hist.value.slice()
    const scrollMul = 6
    for (let i = 0; i < scrollMul; i++) next.push(frame)
    if (next.length > maxHistory) {
      hist.value = next.slice(next.length - maxHistory)
    } else {
      hist.value = next
    }
  },
  { deep: true }
)

function heatColor(v: number) {
  const clamp = (x: number) => Math.max(0, Math.min(1, x))
  const enhance = (x: number, c = 1.45, pivot = 0.55) => clamp((x - pivot) * c + pivot)
  const t = enhance(clamp(v))
  const mid = 0.6
  const lerp = (a: number, b: number, t2: number) => Math.round(a + (b - a) * t2)
  if (t <= mid) {
    const u = t / mid
    return `rgb(${lerp(36, 235, u)},${lerp(6, 26, u)},${lerp(13, 80, u)})`
  }
  const u = (t - mid) / (1 - mid)
  return `rgb(${lerp(235, 255, u)},${lerp(26, 214, u)},${lerp(80, 225, u)})`
}

const fsIcon = computed(() => (props.themeName === 'dark' ? fullscreenLight : fullscreenDark))

const trackWidth = computed(() => Math.max(100, effectiveWidth.value - 60))
const fillWidth = computed(() => Math.max(0, Math.min(trackWidth.value, Math.round(rmsSmoothed.value * trackWidth.value))))
</script>

<template>
  <div
    class="viz-panel"
    :style="{ width: `${effectiveWidth}px` }"
    ref="containerRef"
    @mouseenter="hovered = true"
    @mouseleave="hovered = false"
  >
    <div class="viz-stage">
      <svg
        :width="effectiveWidth"
        :height="effectiveHeight"
        :style="{
          background: theme.panelBg ?? theme.panel ?? '#1a0a0f',
          border: '1px solid #1d2a3a',
          borderRadius: '6px'
        }"
        @click="emit('toggle')"
      >
        <WaveViz
          v-if="mode === 'wave'"
          :width="effectiveWidth"
          :height="effectiveHeight"
          :samples="smoothSamples"
          :gain="gain"
          :grid-color="theme.grid"
          :line-color="theme.line ?? theme.accent"
        />
        <BarsViz
          v-else-if="mode === 'bars'"
          :width="effectiveWidth"
          :height="effectiveHeight"
          :samples="smoothSamples"
          :gain="gain"
          :bar-color="theme.line ?? theme.accent"
          :bars="Math.max(32, Math.min(2048, Math.floor((effectiveWidth + gap) / 3)))"
        />
        <WaterfallViz
          v-else
          :width="effectiveWidth"
          :height="effectiveHeight"
          :bands="bands"
          :gap="gap"
          :cell="Math.max(3, Math.min(12, Math.floor(effectiveHeight / bands) - gap))"
          :history="hist"
          :heat-color="heatColor"
          :override-offset-y="0"
        />
      </svg>
      <button
        v-if="!isFullscreen"
        class="fs-btn"
        :style="{
          opacity: hovered ? 1 : 0,
          transform: hovered ? 'translateY(0) scale(1)' : 'translateY(-4px) scale(0.95)'
        }"
        @click.stop="enterFullscreen"
        title="全屏"
      >
        <img :src="fsIcon" alt="fullscreen" width="24" height="24" />
      </button>
    </div>
    <div v-if="!hideRms" class="rms-row" :style="{ width: `${effectiveWidth}px` }">
      <div class="track" :style="{ background: '#3a0b17' }">
        <div class="fill" :style="{ width: `${fillWidth}px`, background: theme.accent }"></div>
      </div>
      <span>RMS {{ Math.round(rmsSmoothed * 100) }}%</span>
    </div>
  </div>
</template>

<style scoped>
.viz-panel {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.viz-stage {
  position: relative;
  width: 100%;
  margin-top: 40px;
}

.fs-btn {
  position: absolute;
  right: 8px;
  top: 8px;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 50%;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: opacity 0.18s ease, transform 0.18s ease;
}

.fs-btn img {
  width: 20px;
  height: 20px;
  pointer-events: none;
}

.rms-row {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 12px;
  color: #6b829e;
  margin-top: -5px;
}

.track {
  flex: 1;
  height: 8px;
  border-radius: 999px;
  overflow: hidden;
}

.fill {
  height: 100%;
  border-radius: 999px;
  transition: width 120ms ease;
}
</style>
