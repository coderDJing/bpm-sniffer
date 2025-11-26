<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import VizPanel from './bpm/VizPanel.vue'
import refreshIcon from '../assets/bpm/refresh.png'
import sunIcon from '../assets/bpm/sun.png'
import moonIcon from '../assets/bpm/moon.png'
import pinIcon from '../assets/bpm/thumbtack.png'
import floatingIcon from '../assets/bpm/floatingWindow.png'

type VizMode = 'wave' | 'bars' | 'waterfall'
type AudioViz = { samples: number[]; rms: number }
type BeatShape = {
  attack: number
  decay: number
  wavelets: number
  bend: number
  offset: number
  grit: number
}

const themeName = ref<'dark' | 'light'>('dark')
const alwaysOnTop = ref(false)
const floating = ref(false)
const vizMode = ref<VizMode>('wave')
const refreshSpin = ref(false)

const SAMPLE_COUNT = 512

const makeBeatShape = (): BeatShape => ({
  attack: 0.03 + Math.random() * 0.04,
  decay: 0.16 + Math.random() * 0.22,
  wavelets: 1 + Math.random() * 2,
  bend: 0.2 + Math.random() * 0.6,
  offset: Math.random(),
  grit: 0.3 + Math.random() * 0.7
})

const pulseEnvelope = (phase: number, attack: number, decay: number, curve = 1.5) => {
  let p = phase % 1
  if (p < 0) p += 1
  if (p < attack) {
    const rise = Math.max(0, p / Math.max(attack, 0.001))
    return Math.pow(rise, curve)
  }
  const fall = Math.max(0, (p - attack) / Math.max(decay, 0.001))
  return Math.exp(-fall * (1 + curve * 0.55))
}

const viz = ref<AudioViz>({
  samples: new Array(SAMPLE_COUNT).fill(0),
  rms: 0
})

const floatingCanvasRef = ref<HTMLCanvasElement | null>(null)

const floatingBallSize = 58
const floatingBaseStroke = 1.68
const floatingWidthGain = 2.24
const floatingRadiusGain = 2.56
const floatingInnerRadiusGain = 0.96
const floatingShadowBlur = 9.6
const floatingSegments = 64
const floatingGap = 0.12
const floatingMargin = Math.ceil(floatingShadowBlur + (floatingBaseStroke + floatingWidthGain) / 2 + floatingRadiusGain + 2)
const FLOATING_CANVAS_EDGE = floatingBallSize + floatingMargin * 2

const floatingLabelColor = computed(() => (themeName.value === 'dark' ? '#c8d2df' : '#5c606d'))
function hexToRgba(hex: string, alpha: number) {
  const h = hex.replace('#', '')
  if (h.length !== 3 && h.length !== 6) return `rgba(235,26,80,${alpha})`
  const full = h.length === 3 ? h.split('').map((c) => c + c).join('') : h
  const num = parseInt(full, 16)
  const r = (num >> 16) & 255
  const g = (num >> 8) & 255
  const b = num & 255
  return `rgba(${r},${g},${b},${alpha})`
}

let floatingRafId: number | null = null

function stopFloatingLoop() {
  if (floatingRafId != null) {
    cancelAnimationFrame(floatingRafId)
    floatingRafId = null
  }
}

function renderFloatingFrame() {
  const canvas = floatingCanvasRef.value
  if (!canvas) return
  const ctx = canvas.getContext('2d')
  if (!ctx) return
  const dpr = Math.max(1, Math.floor(window.devicePixelRatio || 1))
  const size = FLOATING_CANVAS_EDGE
  if (canvas.width !== size * dpr || canvas.height !== size * dpr) {
    canvas.width = size * dpr
    canvas.height = size * dpr
    canvas.style.width = `${size}px`
    canvas.style.height = `${size}px`
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
  ctx.clearRect(0, 0, size, size)

  const accent = theme.value.accent ?? theme.value.line ?? '#eb1a50'
  const cx = size / 2
  const cy = size / 2
  const samples = viz.value.samples ?? []
  const rms = viz.value.rms ?? 0
  const silent = rms <= 0.002
  const energyBoost = Math.min(2.2, 0.85 + rms * 2.4)
  const rBase = Math.max(8, floatingBallSize / 2 - floatingBaseStroke / 2 + floatingRadiusGain)

  ctx.save()
  ctx.shadowBlur = 0
  ctx.lineWidth = Math.max(1.1, floatingBaseStroke * (silent ? 0.85 : 0.7))
  ctx.strokeStyle = hexToRgba(accent, silent ? 0.5 : 0.25)
  ctx.beginPath()
  ctx.arc(cx, cy, rBase, 0, Math.PI * 2, false)
  ctx.stroke()
  ctx.restore()

  const seg = (Math.PI * 2) / floatingSegments
  for (let i = 0; i < floatingSegments; i++) {
    let acc = 0
    let cnt = 0
    if (samples.length) {
      const i0 = Math.floor((i / floatingSegments) * samples.length)
      const i1 = Math.floor(((i + 1) / floatingSegments) * samples.length)
      for (let j = i0; j < i1; j++) {
        acc += Math.abs(samples[j] || 0)
        cnt++
      }
    }
    const avg = cnt ? acc / cnt : 0
    const amp = Math.pow(Math.min(1, avg * energyBoost * 1.8), 0.6)
    const alpha = Math.min(1, (silent ? 0.32 : 0.2) + amp * (silent ? 0.55 : 0.75))
    const a0 = -Math.PI / 2 + i * seg + seg * floatingGap * 0.5
    const a1 = a0 + seg * (1 - floatingGap)
    const outerR = rBase + amp * floatingRadiusGain
    ctx.lineWidth = floatingBaseStroke + amp * floatingWidthGain
    ctx.strokeStyle = hexToRgba(accent, alpha)
    ctx.beginPath()
    ctx.arc(cx, cy, outerR, a0, a1, false)
    ctx.stroke()

    const innerR = Math.max(4, rBase - amp * floatingInnerRadiusGain)
    const innerAlpha = Math.min(1, (silent ? 0.25 : 0.16) + amp * 0.55)
    ctx.lineWidth = 1 + amp * 1.1
    ctx.strokeStyle = hexToRgba(accent, innerAlpha)
    ctx.beginPath()
    ctx.arc(cx, cy, innerR, a0, a1, false)
    ctx.stroke()
  }
}

async function startFloatingLoop() {
  await nextTick()
  stopFloatingLoop()
  if (!floating.value || !floatingCanvasRef.value) return
  const tick = () => {
    if (!floating.value || !floatingCanvasRef.value) {
      stopFloatingLoop()
      return
    }
    renderFloatingFrame()
    floatingRafId = requestAnimationFrame(tick)
  }
  tick()
}

const darkTheme = {
  background: '#14060a',
  textPrimary: '#ffffff',
  textSecondary: '#f3a0b3',
  subduedText: '#6b829e',
  accent: '#eb1a50',
  panelBg: '#1a0a0f',
  grid: '#3a0b17',
  line: '#eb1a50',
  confGray: '#9aa3ab',
  panel: '#1a0a0f'
}

const lightTheme = {
  background: '#fff4f7',
  textPrimary: '#1b0a10',
  textSecondary: '#b21642',
  subduedText: '#5c6c7a',
  accent: '#eb1a50',
  panelBg: '#ffe8ee',
  grid: '#ffd0db',
  line: '#eb1a50',
  confGray: '#8a8f96',
  panel: '#ffe8ee'
}

const theme = computed(() => (themeName.value === 'dark' ? darkTheme : lightTheme))

const displayBpm = computed(() => '124')
const metaText = '节拍稳定 · 置信度：'
const confText = '稳定'
const confColor = computed(() => theme.value.textPrimary)
const bpmColor = computed(() => theme.value.textPrimary)

function toggleTheme() {
  themeName.value = themeName.value === 'dark' ? 'light' : 'dark'
}

function togglePin() {
  alwaysOnTop.value = !alwaysOnTop.value
}

function toggleFloating() {
  floating.value = !floating.value
}

function handleRefresh() {
  if (refreshSpin.value) return
  refreshSpin.value = true
  setTimeout(() => {
    refreshSpin.value = false
  }, 420)
}

function cycleVizMode() {
  vizMode.value = vizMode.value === 'wave' ? 'bars' : vizMode.value === 'bars' ? 'waterfall' : 'wave'
}

let vizTimer: number | null = null

const getNow = () => (typeof performance !== 'undefined' ? performance.now() : Date.now())
let driftPhase = Math.random() * Math.PI * 2
let rushPhase = Math.random() * Math.PI * 2
let beatPhase = Math.random()
let beatEnergy = 0.3
let lastTick = getNow()
let glitchEnergy = 0
let dropoutTimer = 0
let dropoutActive = false
let rumblePhase = Math.random() * Math.PI * 2
let warpPhase = Math.random() * Math.PI * 2
let beatShape = makeBeatShape()
let centerBias = 0

function updateFakeStream() {
  const phaseBefore = beatPhase
  const now = getNow()
  const dt = Math.max(0.016, Math.min(0.08, (now - lastTick) / 1000))
  lastTick = now

  const bpmDrift = 122 + Math.sin(now * 0.0002) * 6 + (Math.random() - 0.5) * 3
  beatPhase += (bpmDrift / 60) * dt
  if (beatPhase >= 1) {
    beatPhase %= 1
    beatEnergy = 1.1 + Math.random() * 0.6
    beatShape = makeBeatShape()
  } else {
    beatEnergy = Math.max(0, beatEnergy - dt * (3 + Math.random()))
  }

  driftPhase += dt * (2.2 + Math.sin(now * 0.0006) * 1.1)
  rushPhase += dt * (5.5 + Math.sin(now * 0.0011) * 1.8)
  rumblePhase += dt * (18 + Math.sin(now * 0.0008) * 9)
  warpPhase += dt * (0.6 + Math.sin(now * 0.00012) * 0.4)

  glitchEnergy = Math.max(0, glitchEnergy - dt * (1.6 + Math.random() * 0.6))
  if (Math.random() < dt * 0.9) {
    glitchEnergy = Math.max(glitchEnergy, 0.9 + Math.random() * 0.8)
  }

  if (!dropoutActive && Math.random() < dt * 0.05) {
    dropoutActive = true
    dropoutTimer = 0.18 + Math.random() * 0.4
  }
  if (dropoutActive) {
    dropoutTimer -= dt
    if (dropoutTimer <= 0) {
      dropoutActive = false
      dropoutTimer = 0
    }
  }

  let phaseDelta = beatPhase - phaseBefore
  if (phaseDelta < 0) phaseDelta += 1

  const next: number[] = new Array(SAMPLE_COUNT)
  let bias = centerBias
  for (let i = 0; i < SAMPLE_COUNT; i++) {
    const u = i / SAMPLE_COUNT
    const t = driftPhase + u * 12
    const localBeat = (phaseBefore + phaseDelta * u) % 1
    const mainPulse = pulseEnvelope(localBeat, beatShape.attack, beatShape.decay, 1.3 + beatShape.wavelets * 0.25)
    const ghostPulse = pulseEnvelope(
      (localBeat + beatShape.offset) % 1,
      beatShape.attack * 0.65,
      beatShape.decay * 0.5,
      1 + beatShape.wavelets * 0.3
    )
    const gate = Math.min(1.2, mainPulse * (0.85 + beatEnergy * 0.25) + ghostPulse * 0.45 + 0.08)
    const gateFloor = Math.max(0.15, gate)
    const envelope = (0.42 + Math.sin(now * 0.0005 + warpPhase + u * 2) * 0.2) * gateFloor
    const base = Math.sin(t + Math.sin(t * 0.2) * beatShape.bend) * envelope
    const harmonic =
      Math.sin(t * (0.52 + Math.sin(u * 3 + rushPhase) * 0.08) + Math.sin(t * 0.08) * 2.3) *
      (0.22 + gate * 0.35)
    const jagged =
      Math.sin(rushPhase + u * (40 + Math.sin(now * 0.0009) * 16) + Math.sin(t) * 3.4) *
      (0.08 + gate * 0.24)
    const rumble =
      Math.sin(rumblePhase + u * (70 + Math.sin(warpPhase + u * 6) * 28)) * (0.05 + gate * 0.18)
    const microSwing = Math.sin(u * (120 + beatShape.wavelets * 32) + rushPhase * 0.5) * (0.04 + ghostPulse * 0.18)
    const burst = Math.sin(u * 90 + rushPhase * 1.5 + mainPulse * 14) * glitchEnergy * (0.12 + mainPulse * 0.45)
    const noise =
      (Math.random() - 0.5) *
      (0.18 + glitchEnergy * 0.4 + gate * 0.35 + beatShape.grit * 0.08 + (dropoutActive ? 0.12 : 0))

    let beatDelta = localBeat
    if (beatDelta > 0.5) beatDelta -= 1
    if (beatDelta < -0.5) beatDelta += 1
    const beatWidth = Math.max(0.03, 0.05 + Math.sin(now * 0.0004 + rushPhase) * 0.02)
    const beatPulse =
      Math.exp(-(beatDelta * beatDelta) / (2 * beatWidth * beatWidth)) *
      beatEnergy *
      (0.8 + mainPulse * 0.6 + ghostPulse * 0.3)

    const dropoutWarp = dropoutActive
      ? (Math.sin(u * 8 + now * 0.004) * 0.35 + (Math.random() - 0.5) * 0.3) * (0.5 + gate * 0.4)
      : 0
    const tonal = (base + harmonic + jagged + rumble + microSwing) * (dropoutActive ? 0.55 : 1)
    const rawVal = tonal + beatPulse + burst + noise + dropoutWarp

    bias = bias * 0.985 + rawVal * 0.015
    const centered = rawVal - bias * (dropoutActive ? 0.35 : 0.85)
    const shaped = centered * (0.35 + gate)

    next[i] = Math.max(-1, Math.min(1, shaped))
  }
  centerBias = bias

  const rms = Math.sqrt(next.reduce((acc, cur) => acc + cur * cur, 0) / next.length)
  viz.value = { samples: next, rms }
}

watch(
  () => floating.value,
  (enabled) => {
    if (enabled) {
      startFloatingLoop()
    } else {
      stopFloatingLoop()
    }
  }
)

watch(floatingCanvasRef, (canvas) => {
  if (canvas && floating.value) {
    startFloatingLoop()
  }
})

onMounted(() => {
  updateFakeStream()
  vizTimer = window.setInterval(updateFakeStream, 40)
})

onUnmounted(() => {
  if (vizTimer) clearInterval(vizTimer)
  stopFloatingLoop()
})
</script>

<template>
  <div class="demo-window" :class="{ floating }" :style="{ background: theme.background, color: theme.textPrimary }">
    <div v-if="!floating" class="panel-content">
      <div class="title">BPM</div>
      <div class="bpm-display" :style="{ color: bpmColor }">
        {{ displayBpm }}
      </div>
      <div class="meta-line">
        {{ metaText }}
        <span class="conf" :style="{ color: confColor }">{{ confText }}</span>
      </div>
      <VizPanel
        class="viz-panel-shell"
        :theme="theme"
        :hide-rms="false"
        :viz="viz"
        :mode="vizMode"
        :theme-name="themeName"
        :width="350"
        :base-height="150"
        @toggle="cycleVizMode"
      />
      <div class="actions">
        <button class="icon-btn" title="刷新" @click="handleRefresh">
          <img
            :src="refreshIcon"
            alt="刷新"
            :style="{
              transform: refreshSpin ? 'rotate(360deg)' : 'rotate(0deg)',
              transition: 'transform 360ms ease'
            }"
          />
        </button>
        <button class="icon-btn" title="切换主题" @click="toggleTheme">
          <span class="theme-icons">
            <img :src="sunIcon" alt="日间" :class="{ active: themeName === 'light' }" />
            <img :src="moonIcon" alt="夜间" :class="{ active: themeName === 'dark' }" />
          </span>
        </button>
        <button class="icon-btn" :title="alwaysOnTop ? '已置顶' : '置顶'" @click="togglePin">
          <img :src="pinIcon" alt="置顶" :class="{ pinned: alwaysOnTop }" />
        </button>
        <button class="icon-btn" :title="floating ? '悬浮中' : '悬浮球'" @click="toggleFloating">
          <img :src="floatingIcon" alt="悬浮球" :class="{ active: floating }" />
        </button>
      </div>
    </div>
    <div v-else class="floating-shell">
      <div
        class="floating-ball"
        :style="{ width: FLOATING_CANVAS_EDGE + 'px', height: FLOATING_CANVAS_EDGE + 'px' }"
        title="双击返回面板"
        @dblclick.stop.prevent="toggleFloating"
      >
        <canvas ref="floatingCanvasRef" class="floating-canvas"></canvas>
        <div
          class="floating-core"
          :style="{
            width: floatingBallSize + 'px',
            height: floatingBallSize + 'px',
            background: theme.panelBg ?? theme.panel ?? theme.background
          }"
        >
          <span class="floating-core-text" :style="{ color: bpmColor }">{{ displayBpm }}</span>
          <span class="floating-core-label" :style="{ color: floatingLabelColor }">BPM</span>
        </div>
      </div>
      <div class="floating-hint">双击返回面板</div>
    </div>
  </div>
</template>

<style scoped>
.demo-window {
  width: 380px;
  height: 420px;
  border-radius: 5px;
  padding: 50px 10px 24px;
  position: relative;
  box-shadow:
    0 25px 60px rgba(0, 0, 0, 0.55),
    inset 0 1px 0 rgba(255, 255, 255, 0.06);
  border: 1px solid #1d2a3a;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0;
}

.panel-content {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
}

.demo-window.floating {
  width: 220px;
  height: 220px;
  padding: 0;
  border: none;
  box-shadow: none;
  background: transparent;
}

.floating-shell {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 16px;
}

.floating-ball {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}

.floating-canvas {
  position: absolute;
  inset: 0;
  pointer-events: none;
  filter: none;
}

.floating-core {
  position: relative;
  border-radius: 50%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 2px;
  box-shadow: none;
  letter-spacing: 1px;
}

.floating-core-text {
  font-size: 22px;
  font-weight: 700;
  line-height: 1;
}

.floating-core-label {
  font-size: 9px;
  letter-spacing: 1.8px;
}

.floating-hint {
  font-size: 12px;
  color: #aeb7c4;
  letter-spacing: 1px;
}

.actions {
  position: absolute;
  top: 12px;
  right: 14px;
  display: flex;
  gap: 8px;
}

.icon-btn {
  width: 26px;
  height: 26px;
  border: none;
  background: transparent;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}

.icon-btn img {
  width: 22px;
  height: 22px;
  user-select: none;
}

.icon-btn img.pinned {
  transform: rotate(-45deg);
  transition: transform 150ms ease;
}

.icon-btn img.active {
  filter: drop-shadow(0 0 6px rgba(235, 26, 80, 0.6));
}

.theme-icons {
  position: relative;
  width: 22px;
  height: 22px;
}

.theme-icons img {
  position: absolute;
  inset: 0;
  opacity: 0;
  transition: opacity 160ms ease, transform 160ms ease;
}

.theme-icons img.active {
  opacity: 1;
  transform: scale(1);
}

.theme-icons img:not(.active) {
  transform: scale(0.7);
}

.title {
  font-size: 18px;
  letter-spacing: 0;
  font-weight: 600;
  color: #eb1a50;
  margin: 0;
}

.bpm-display {
  font-size: 96px;
  font-weight: 700;
  letter-spacing: 2px;
  height: 100px;
  line-height: 100px;
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  user-select: none;
}

.meta-line {
  font-size: 14px;
  color: v-bind('theme.textSecondary');
  text-align: center;
  padding-top: 10px;
}

.meta-line .conf {
  font-weight: 600;
}

.viz-panel-shell {
  margin-top: auto;
  margin-bottom: 7px;
  width: 100%;
  display: flex;
  justify-content: center;
}

</style>
