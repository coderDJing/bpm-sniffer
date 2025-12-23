import React, { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import thumbtack from './assets/thumbtack.png'
import sun from './assets/sun.png'
import moon from './assets/moon.png'
import refresh from './assets/refresh.png'
import floatingWindow from './assets/floatingWindow.png'
import keyCamelotIcon from './assets/A.png'
import keyNoteIcon from './assets/C.png'
// @ts-ignore: optional plugin at runtime
import { check } from '@tauri-apps/plugin-updater'
import { getVersion } from '@tauri-apps/api/app'
import { t, tn } from './i18n'
import AboutWindow from './component/AboutWindow'
import FloatBall from './component/FloatBall'
import LogsWindow from './component/LogsWindow'
import VizPanel from './component/VizPanel'
import { AudioViz, DisplayBpm, DisplayKey } from './types'

export default function App() {
  const [route, setRoute] = useState<string>(typeof window !== 'undefined' ? window.location.hash : '')
  const [bpm, setBpm] = useState<number | null>(null)
  const [conf, setConf] = useState<number | null>(null)
  const [state, setState] = useState<DisplayBpm['state']>('analyzing')
  const [keyNote, setKeyNote] = useState<string | null>(null)
  const [keyCamelot, setKeyCamelot] = useState<string | null>(null)
  const [keyConf, setKeyConf] = useState<number | null>(null)
  const [keyState, setKeyState] = useState<DisplayKey['state']>('analyzing')
  const [keyMode, setKeyMode] = useState<'note' | 'camelot'>('note')
  const [keyIconTick, setKeyIconTick] = useState(0)
  const [alwaysOnTop, setAlwaysOnTop] = useState<boolean>(false)
  const [viz, setViz] = useState<AudioViz | null>(null)
  const [vizMode, setVizMode] = useState<'wave'|'bars'|'waterfall'>('wave')
  const [themeName, setThemeName] = useState<'dark' | 'light'>('dark')
  const [appVersion, setAppVersion] = useState<string>('')
  const [updateReady, setUpdateReady] = useState<boolean>(false)
  const [manualMode, setManualMode] = useState<boolean>(false)
  const [manualBpm, setManualBpm] = useState<number | null>(null)
  const manualModeRef = useRef<boolean>(false)
  const mqlCleanupRef = useRef<null | (() => void)>(null)
  const keyHoldRef = useRef<{ note: string | null, camelot: string | null }>({ note: null, camelot: null })
  const keyConfEmaRef = useRef<number | null>(null)
  const keyConfKeyRef = useRef<string | null>(null)
  // 高亮锁：当某个值在高置信度下被高亮后，如果之后收到同值但低置信度的数据，仍保持高亮，直到值发生变化
  const bpmRef = useRef<number | null>(null)
  const highlightLockRef = useRef<{ locked: boolean, bpm: number | null }>({ locked: false, bpm: null })
  // 低置信度同值连续计数：当灰显同值连续达到阈值（如5）时，自动视为需要高亮
  const lowConfStreakRef = useRef<{ bpm: number | null, count: number }>({ bpm: null, count: 0 })
  const lowConfPromoteThreshold = 5
  const keyConfAlpha = 0.5
  const manualTapTimesRef = useRef<number[]>([])

  useEffect(() => {
    manualModeRef.current = manualMode
  }, [manualMode])

  const darkTheme = {
    background: '#14060a',
    textPrimary: '#ffffff',
    textSecondary: '#f3a0b3',
    subduedText: '#6b829e',
    accent: '#eb1a50',
    panelBg: '#1a0a0f',
    grid: '#3a0b17',
    line: '#eb1a50',
    track: '#3a0b17',
    confGray: '#9aa3ab'
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
    track: '#ffd0db',
    confGray: '#8a8f96'
  }
  const theme = themeName === 'dark' ? darkTheme : lightTheme

  // 静音计时：10 秒无声则触发一次前端刷新（归零）
  const lastNonSilentAtRef = useRef<number>(Date.now())
  const silenceTriggeredRef = useRef<boolean>(false)
  // “等待声音”显示去抖：仅当连续静音超过 WAIT_MS 才显示
  const [showWaiting, setShowWaiting] = useState<boolean>(false)
  const showWaitingRef = useRef<boolean>(false)
  const SILENT_LABEL_WAIT_MS = 1500
  const SILENT_ENTER_THR = 0.0 // 后端已把极低电平折算为 0
  const SILENT_EXIT_THR = 0.01 // 退出等待需要略高一些，形成回滞，减少抖动

  function resetManualMode() {
    setManualMode(false)
    setManualBpm(null)
    manualTapTimesRef.current = []
  }

  function handleManualPointerDown(e: React.PointerEvent) {
    e.preventDefault()
    e.stopPropagation()
    if (e.button === 2) {
      if (manualMode) {
        resetManualMode()
      }
      return
    }
    if (e.button !== 0) return
    const now = Date.now()
    const filtered = manualTapTimesRef.current.filter(ts => now - ts <= 8000)
    filtered.push(now)
    while (filtered.length > 8) filtered.shift()
    manualTapTimesRef.current = filtered
    if (!manualMode) {
      setManualMode(true)
      const autoBase = bpmRef.current ?? bpm ?? null
      if (autoBase != null) {
        setManualBpm(autoBase)
      }
    }
    if (manualMode && filtered.length === 1) {
      setManualBpm(null)
    }
    if (filtered.length >= 2) {
      const intervals: number[] = []
      for (let i = 1; i < filtered.length; i++) {
        const span = filtered[i] - filtered[i - 1]
        if (span > 0) intervals.push(span)
      }
      if (intervals.length) {
        const sum = intervals.reduce((acc, cur) => acc + cur, 0)
        const avg = sum / intervals.length
        if (avg > 0) {
          const bpmValue = Math.min(300, Math.max(30, 60000 / avg))
          setManualBpm(bpmValue)
        }
      }
    }
  }

  // 归零即刷新：抽取公共方法，供按钮与静音超时复用
  async function doRefresh(options?: { keepManualMode?: boolean }) {
    if (refreshSpin) return
    setRefreshSpin(true)
    const keepManualMode = options?.keepManualMode ?? false
    try {
      // 清空前端可见状态
      if (!keepManualMode) {
        resetManualMode()
      }
      setBpm(null); bpmRef.current = null
      setConf(null)
      setState('analyzing')
      setKeyNote(null)
      setKeyCamelot(null)
      setKeyConf(null)
      setKeyState('analyzing')
      keyHoldRef.current = { note: null, camelot: null }
      setViz(null)
      highlightLockRef.current = { locked: false, bpm: null }
      lowConfStreakRef.current = { bpm: null, count: 0 }
      // 通知后端软重置
      try { await invoke('reset_backend') } catch {}
    } finally {
      // 启动一次 360° 顺时针旋转动画
      setTimeout(() => setRefreshSpin(false), 420)
    }
  }

  useEffect(() => {
    let removeListener: (() => void) | null = null
    let removeMql: (() => void) | null = null
    ;(async () => {
      try {
        // 初始化主题：优先读取用户偏好，否则跟随系统
        try {
          const saved = localStorage.getItem('bpm_theme')
          const mql = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)')
          if (saved === 'light' || saved === 'dark') {
            setThemeName(saved as 'light' | 'dark')
          } else if (mql) {
            setThemeName(mql.matches ? 'dark' : 'light')
            const handler = (e: MediaQueryListEvent) => setThemeName(e.matches ? 'dark' : 'light')
            if (mql.addEventListener) mql.addEventListener('change', handler)
            else if ((mql as any).addListener) (mql as any).addListener(handler)
            removeMql = () => {
              if (mql.removeEventListener) mql.removeEventListener('change', handler)
              else if ((mql as any).removeListener) (mql as any).removeListener(handler)
            }
            mqlCleanupRef.current = removeMql
          }
        } catch {}

        // 恢复置顶状态 / 可视化模式（窗口大小不做持久化）
        try {
          const savedTop = localStorage.getItem('bpm_on_top')
          if (savedTop === '1' || savedTop === 'true') {
            await invoke('set_always_on_top', { onTop: true })
            setAlwaysOnTop(true)
          }
          const savedViz = localStorage.getItem('bpm_viz_mode')
          if (savedViz === 'wave' || savedViz === 'bars' || savedViz === 'waterfall') {
            setVizMode(savedViz as 'wave'|'bars'|'waterfall')
          }
          const savedKeyMode = localStorage.getItem('key_mode')
          if (savedKeyMode === 'note' || savedKeyMode === 'camelot') {
            setKeyMode(savedKeyMode as 'note'|'camelot')
          }
        } catch {}

        const unlistenA = await listen<DisplayBpm>('bpm_update', (e) => {
          const res = e.payload
          // 计算即将展示的 BPM（后端传 0 则沿用上一值），并基于“显示整数值”进行比较/锁定
          const currentDisplayed = bpmRef.current // 原始值
          const nextBpm = res.bpm > 0 ? res.bpm : currentDisplayed // 原始值
          const currentDisplayedInt = currentDisplayed != null ? Math.round(currentDisplayed) : null
          const incomingInt = res.bpm > 0 ? Math.round(res.bpm) : null
          const nextDisplayInt = nextBpm != null ? Math.round(nextBpm) : null

          // 若“显示整数值”发生变化，取消之前的高亮锁并重置计数
          if (incomingInt != null && currentDisplayedInt != null && incomingInt !== currentDisplayedInt) {
            highlightLockRef.current = { locked: false, bpm: null }
            lowConfStreakRef.current = { bpm: null, count: 0 }
          }
          // 若当前置信度足够高，则为该值加锁（保持高亮）
          if (res.confidence >= 0.5 && nextDisplayInt != null) {
            highlightLockRef.current = { locked: true, bpm: nextDisplayInt }
            // 高置信度到来时重置低置信度连续计数
            lowConfStreakRef.current = { bpm: null, count: 0 }
          }

          // 若当前置信度较低，但连续 5 次报告同一个有效 BPM，则也视为需要高亮
          if (res.confidence < 0.5) {
            if (incomingInt != null && currentDisplayedInt != null && incomingInt === currentDisplayedInt) {
              if (lowConfStreakRef.current.bpm === incomingInt) {
                lowConfStreakRef.current.count += 1
              } else {
                lowConfStreakRef.current = { bpm: incomingInt, count: 1 }
              }
              if (lowConfStreakRef.current.count >= lowConfPromoteThreshold && nextDisplayInt != null) {
                highlightLockRef.current = { locked: true, bpm: nextDisplayInt }
              }
            } else if (incomingInt != null) {
              // 低置信度但值不同或与当前显示不一致，重置为新值计数起点
              lowConfStreakRef.current = { bpm: incomingInt, count: 1 }
            }
          }

          setConf(res.confidence)
          setState(res.state)
          // 后端已做过滤：收到即显示；为0则保留上一次
          if (res.bpm > 0) {
            setBpm(res.bpm)
            bpmRef.current = res.bpm
          }
        })
        const unlistenK = await listen<DisplayKey>('key_update', (e) => {
          const res = e.payload
          const incomingNote = res.key
          const incomingCamelot = res.camelot
          const valid = incomingNote !== '-' && incomingCamelot !== '-'
          const keyId = valid ? `${incomingNote}|${incomingCamelot}` : '-'
          let nextKeyConf = res.confidence
          const shouldReset = !valid || res.state === 'analyzing' || res.state === 'atonal' || keyConfKeyRef.current !== keyId
          if (shouldReset) {
            keyConfEmaRef.current = nextKeyConf
            keyConfKeyRef.current = keyId
          } else {
            const prev = keyConfEmaRef.current ?? nextKeyConf
            nextKeyConf = prev * (1 - keyConfAlpha) + nextKeyConf * keyConfAlpha
            keyConfEmaRef.current = nextKeyConf
          }
          setKeyConf(nextKeyConf)
          setKeyState(res.state)
          if (res.state === 'tracking' && valid && res.confidence >= 0.55) {
            keyHoldRef.current = { note: incomingNote, camelot: incomingCamelot }
            setKeyNote(incomingNote)
            setKeyCamelot(incomingCamelot)
            return
          }
          if (res.state === 'uncertain') {
            if (keyHoldRef.current.note && keyHoldRef.current.camelot) {
              setKeyNote(keyHoldRef.current.note)
              setKeyCamelot(keyHoldRef.current.camelot)
            } else {
              setKeyNote('-')
              setKeyCamelot('-')
            }
            return
          }
          keyHoldRef.current = { note: null, camelot: null }
          setKeyNote('-')
          setKeyCamelot('-')
        })
        const unlistenD = await listen<AudioViz>('viz_update', async (e) => {
          const payload = e.payload as any as AudioViz
          setViz(payload)
          // 基于 RMS 判断是否静音
          const rms = payload?.rms ?? 0
          const nowTs = Date.now()
          // 去抖逻辑：
          if (rms > SILENT_EXIT_THR) {
            lastNonSilentAtRef.current = nowTs
            silenceTriggeredRef.current = false
            if (showWaitingRef.current) { setShowWaiting(false); showWaitingRef.current = false }
          } else {
            const SILENT_TIMEOUT_MS = 10000
            if (!silenceTriggeredRef.current && (nowTs - lastNonSilentAtRef.current >= SILENT_TIMEOUT_MS)) {
              silenceTriggeredRef.current = true
              await doRefresh({ keepManualMode: manualModeRef.current })
            }
            // 连续静音达到阈值才显示“等待声音”
            if (!showWaitingRef.current && rms <= SILENT_ENTER_THR && (nowTs - lastNonSilentAtRef.current >= SILENT_LABEL_WAIT_MS)) {
              setShowWaiting(true); showWaitingRef.current = true
            }
          }
        })
        removeListener = () => { if (removeMql) removeMql(); unlistenA(); unlistenK(); unlistenD() }

        // 仅靠后端初始化语言；前端不再覆盖后端语言
        await invoke('start_capture')
        // 获取应用版本号
        try { const v = await getVersion(); setAppVersion(v) } catch {}
        // 静默检查并下载更新（可达性自动选择端点）
        try {
          const update = await check()
          if (update?.available) {
            await update.downloadAndInstall()
            // 轻提示：已更新完毕，下次重启生效（需要用户手动关闭）
            setUpdateReady(true)
            // 可选：不自动重启，保留当前会话；如需立即生效可调用 relaunch()
          }
        } catch {}
      } catch (err) { console.error(tn('[启动] 错误', '[BOOT] error'), err) }
    })()

    return () => { if (removeListener) removeListener() }
  }, [])

  // 跨窗口主题同步：监听 localStorage 变化
  useEffect(() => {
    function onStorage(e: StorageEvent) {
      if (e.key === 'bpm_theme') {
        const v = e.newValue
        if (v === 'dark' || v === 'light') setThemeName(v)
      } else if (e.key === 'key_mode') {
        const v = e.newValue
        if (v === 'note' || v === 'camelot') setKeyMode(v)
      }
    }
    window.addEventListener('storage', onStorage)
    return () => window.removeEventListener('storage', onStorage)
  }, [])

  // 路由：用于关于独立窗口（/#about）
  useEffect(() => {
    const handler = () => setRoute(window.location.hash)
    window.addEventListener('hashchange', handler)
    return () => window.removeEventListener('hashchange', handler)
  }, [])

  // 全局禁用右键与拖拽；选择在 #logs 允许，其他页面禁用
  useEffect(() => {
    const prevent = (e: Event) => { e.preventDefault(); e.stopPropagation() }
    const preventSel = (e: Event) => { e.preventDefault(); e.stopPropagation() }
    window.addEventListener('contextmenu', prevent, { capture: true })
    window.addEventListener('dragstart', prevent, { capture: true })
    // 路由变化时，控制是否禁止选择
    if (route !== '#logs') {
      window.addEventListener('selectstart', preventSel, { capture: true })
    } else {
      window.removeEventListener('selectstart', preventSel, { capture: true } as any)
    }
    return () => {
      window.removeEventListener('contextmenu', prevent, { capture: true } as any)
      window.removeEventListener('dragstart', prevent, { capture: true } as any)
      window.removeEventListener('selectstart', preventSel, { capture: true } as any)
    }
  }, [route])


  function toggleTheme() {
    const next = themeName === 'dark' ? 'light' : 'dark'
    setThemeName(next)
    try { localStorage.setItem('bpm_theme', next) } catch {}
    if (mqlCleanupRef.current) { mqlCleanupRef.current(); mqlCleanupRef.current = null }
  }

  const baseLabel = state === 'tracking' ? t('state_tracking') : state === 'analyzing' ? t('state_analyzing') : t('state_uncertain')
  const baseConfLabel = conf == null ? '—' : conf >= 0.75 ? t('conf_stable') : conf >= 0.5 ? t('conf_medium') : t('conf_unstable')
  const baseConfColor = conf == null ? theme.confGray : (conf >= 0.5 ? theme.textPrimary : theme.confGray)
  const keyLabel = showWaiting
    ? t('state_waiting_audio')
    : keyState === 'tracking'
      ? t('key_state_tracking')
      : keyState === 'analyzing'
        ? t('key_state_analyzing')
        : keyState === 'uncertain'
          ? t('key_state_uncertain')
          : t('key_state_atonal')
  const keyConfLabel = showWaiting ? '—' : (keyConf == null ? '—' : keyConf >= 0.75 ? t('conf_stable') : keyConf >= 0.5 ? t('conf_medium') : t('conf_unstable'))
  const keyConfColor = showWaiting ? theme.confGray : (keyConf == null ? theme.confGray : (keyConf >= 0.5 ? theme.textPrimary : theme.confGray))
  // 当高亮锁生效且锁定的值与当前显示值一致时，始终使用高亮颜色
  const autoDisplayedBpm = (bpm == null ? bpmRef.current : bpm)
  const currentDisplayedBpm = manualMode ? manualBpm : autoDisplayedBpm
  const currentDisplayedIntForColor = currentDisplayedBpm != null ? Math.round(currentDisplayedBpm) : null
  const isLockedHighlight = !manualMode && !!(highlightLockRef.current.locked && highlightLockRef.current.bpm != null && highlightLockRef.current.bpm === currentDisplayedIntForColor)
  const confLabel = showWaiting ? '—' : (isLockedHighlight ? t('conf_stable') : baseConfLabel)
  const confColor = showWaiting ? theme.confGray : (isLockedHighlight ? theme.textPrimary : baseConfColor)
  const bpmColor = manualMode
    ? theme.accent
    : (isLockedHighlight ? theme.textPrimary : (conf == null ? theme.confGray : (conf >= 0.5 ? theme.textPrimary : theme.confGray)))
  // 状态标签：当静音去抖成立时显示“等待声音”；其余规则保持不变
  const label = showWaiting ? t('state_waiting_audio') : (state === 'analyzing' ? t('state_analyzing') : (isLockedHighlight ? t('state_tracking') : baseLabel))
  const displayBpm = currentDisplayedBpm != null ? Math.round(currentDisplayedBpm) : 0
  const displayKey = (keyMode === 'camelot' ? keyCamelot : keyNote) || '-'
  const keyStable = keyState === 'tracking' && keyConf != null && keyConf >= 0.55 && displayKey !== '-'
  const keyColor = keyStable ? theme.textPrimary : theme.confGray
  const bpmBright = !manualMode && (isLockedHighlight || (conf != null && conf >= 0.5))
  const slashColor = (bpmBright && keyStable) ? theme.textPrimary : theme.confGray
  const keyModeIcon = keyMode === 'camelot' ? keyCamelotIcon : keyNoteIcon
  const viewH = typeof window !== 'undefined' ? window.innerHeight : 0
  const layoutDensity = viewH < 320 ? 'tight' : viewH < 380 ? 'compact' : 'normal'
  const mainGap = layoutDensity === 'normal' ? 16 : layoutDensity === 'compact' ? 12 : 8
  const metaPaddingTop = layoutDensity === 'normal' ? 10 : layoutDensity === 'compact' ? 8 : 6
  const metaGap = layoutDensity === 'normal' ? 4 : 3
  const vizMarginBottom = layoutDensity === 'normal' ? 7 : layoutDensity === 'compact' ? 5 : 3

  // 已固定后端为基础模式，无切换

  async function toggleAlwaysOnTop() {
    try {
      const next = !alwaysOnTop
      await invoke('set_always_on_top', { onTop: next })
      setAlwaysOnTop(next)
      try { localStorage.setItem('bpm_on_top', next ? '1' : '0') } catch {}
    } catch (e) {
      console.error(tn('置顶切换失败', 'Toggle pin failed'), e)
    }
  }

  // 自适应隐藏：根据窗口高度动态隐藏部分元素
  const [hideRms, setHideRms] = useState(false)
  const [hideViz, setHideViz] = useState(false)
  const [hideTitle, setHideTitle] = useState(false)
  const [hideMeta, setHideMeta] = useState(false)
  const [hideActions, setHideActions] = useState(false)
  // 轻量触发器：即便各隐藏标志未变化，也强制刷新以更新 VizPanel 宽度
  const [sizeTick, setSizeTick] = useState(0)
  const [refreshSpin, setRefreshSpin] = useState(false)
  useEffect(() => {
    function onResize() {
      const h = window.innerHeight
      const w = window.innerWidth
      // 粗略阈值：根据当前组件布局估算
      setHideRms(h < 360)
      setHideViz(h < 330)
      setHideTitle(h < 200)
      setHideMeta(h < 180)
      setHideActions(w < 380)
      // 强制一次轻量刷新，确保宽度自适应在静态画面时也更新
      setSizeTick((t) => (t + 1) % 1000000)
    }
    onResize()
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])

  // 持久化可视化模式
  useEffect(() => {
    try { localStorage.setItem('bpm_viz_mode', vizMode) } catch {}
  }, [vizMode])
  useEffect(() => {
    try { localStorage.setItem('key_mode', keyMode) } catch {}
  }, [keyMode])

  // 独立关于窗口
  if (route === '#about') {
    return (
      <AboutWindow themeName={themeName} appVersion={appVersion} />
    )
  }
  if (route === '#logs') {
    return (
      <LogsWindow themeName={themeName} />
    )
  }

  if (route === '#float') {
    return (
      <FloatBall
        themeName={themeName}
        bpm={bpmRef.current ?? 0}
        conf={conf}
        viz={viz}
        keyMode={keyMode}
        keyNote={keyNote}
        keyCamelot={keyCamelot}
        keyConf={keyConf}
        keyState={keyState}
        showWaiting={showWaiting}
        onExit={async () => { try { await invoke('exit_floating') } catch {} }}
        isLockedHighlight={isLockedHighlight}
      />
    )
  }

  return (
    <main style={{height:'100vh',display:'flex',flexDirection:'column',alignItems:'center',justifyContent: hideViz ? 'center' : 'flex-start',gap:mainGap,background:theme.background,color:theme.textPrimary,overflow:'hidden'}}>
      {updateReady && (
        <div style={{position:'fixed',left:'50%',transform:'translateX(-50%)',bottom:16,background:theme.panelBg,border:'1px solid #1d2a3a',borderRadius:8,padding:'10px 12px',display:'flex',alignItems:'center',gap:10,zIndex:9999,boxShadow:'0 4px 12px rgba(0,0,0,0.35)',minWidth:'min(360px, calc(100vw - 32px))',maxWidth:'calc(100vw - 32px)',flexWrap:'nowrap',justifyContent:'flex-start'}}>
          <span style={{fontSize:12,color:theme.textPrimary,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis',flex:'1 1 auto',minWidth:0}}>{t('update_ready')}</span>
          <button onClick={() => setUpdateReady(false)} style={{fontSize:12,background:'transparent',border:'1px solid #3a0b17',color:theme.textSecondary,borderRadius:6,cursor:'pointer',padding:'4px 8px',whiteSpace:'nowrap',display:'inline-flex',alignItems:'center',justifyContent:'center'}}>{t('close')}</button>
        </div>
      )}
      <div style={{flex:'1 1 auto', width:'100%', display:'flex', flexDirection:'column', alignItems:'center', justifyContent:'center', gap:0, minHeight: hideViz ? '100vh' : undefined}}>
        {!hideTitle && <h1 style={{margin:0,color:'#eb1a50',fontSize:18}}>{t('app_title')}</h1>}
        <div
          onPointerDown={handleManualPointerDown}
          title={!manualMode ? t('manual_enter_hint') : undefined}
          style={{fontSize:72,fontWeight:700,letterSpacing:2,color:bpmColor,height:'76px',lineHeight:'76px',position:'relative',cursor:'pointer',userSelect:'none',display:'flex',alignItems:'center',justifyContent:'center'}}
        >
          <span style={{color: bpmColor}}>{displayBpm}</span>
          <span style={{color: slashColor}}>/</span>
          <span style={{color: keyColor}}>{displayKey}</span>
        </div>
        {!hideMeta && (
          manualMode ? (
            <div style={{fontSize:14,color:theme.textSecondary,paddingTop:metaPaddingTop,display:'flex',flexDirection:'column',alignItems:'center',gap:metaGap}}>
              <div>{t('manual_exit_hint')}</div>
              <div>
                {keyLabel} · {t('conf_label')}<span style={{color: keyConfColor}}>{keyConfLabel}</span>
              </div>
            </div>
          ) : (
            <div style={{fontSize:14,color:theme.textSecondary,paddingTop:metaPaddingTop,display:'flex',flexDirection:'column',alignItems:'center',gap:metaGap}}>
              <div>
                {label} · {t('conf_label')}<span style={{color: confColor}}>{confLabel}</span>
              </div>
              <div>
                {keyLabel} · {t('conf_label')}<span style={{color: keyConfColor}}>{keyConfLabel}</span>
              </div>
            </div>
          )
        )}
      </div>

      {/* 简易波形可视化 */}
      {!hideViz && (
        <div style={{marginTop:'auto', marginBottom:vizMarginBottom}}>
          <VizPanel theme={theme} hideRms={hideRms} viz={viz} mode={vizMode} onToggle={() => setVizMode(m => m==='wave' ? 'bars' : (m==='bars' ? 'waterfall' : 'wave'))} themeName={themeName} />
        </div>
      )}

      {!hideActions && (
      <div style={{position:'fixed',right:12,top:12,display:'flex',gap:8,alignItems:'center'}}>
        <button
          onClick={async () => {
            await doRefresh()
          }}
          title={t('refresh') || '刷新'}
          style={{
            background:'transparent',
            border:'none',
            padding:0,
            cursor:'pointer',
            width:25,
            height:25,
            display:'flex',
            alignItems:'center',
            justifyContent:'center'
          }}
        >
          <img
            src={refresh}
            alt={t('refresh') || '刷新'}
            width={22}
            height={22}
            draggable={false}
            style={{
              transition:'transform 360ms ease',
              transform: refreshSpin ? 'rotate(360deg)' : 'rotate(0deg)'
            }}
          />
        </button>
        <button
          onClick={toggleTheme}
          title={themeName === 'dark' ? t('theme_toggle_to_light') : t('theme_toggle_to_dark')}
          style={{
            background:'transparent',
            border:'none',
            padding:0,
            cursor:'pointer',
            width:25,
            height:25,
            display:'flex',
            alignItems:'center',
            justifyContent:'center'
          }}
        >
          <div style={{position:'relative', width:25, height:25}}>
            <img
              src={sun}
              alt={t('sun_alt')}
              width={25}
              height={25}
              draggable={false}
              style={{
                position:'absolute',
                left:0,
                top:0,
                opacity: themeName === 'dark' ? 0 : 1,
                transform: themeName === 'dark' ? 'rotate(-90deg) scale(0.85)' : 'rotate(0deg) scale(1)',
                transition:'opacity 180ms ease, transform 220ms ease'
              }}
            />
            <img
              src={moon}
              alt={t('moon_alt')}
              width={25}
              height={25}
              draggable={false}
              style={{
                position:'absolute',
                left:0,
                top:0,
                opacity: themeName === 'dark' ? 1 : 0,
                transform: themeName === 'dark' ? 'rotate(0deg) scale(1)' : 'rotate(90deg) scale(0.85)',
                transition:'opacity 180ms ease, transform 220ms ease'
              }}
            />
          </div>
        </button>
        <button
          onClick={() => {
            setKeyMode(m => m === 'note' ? 'camelot' : 'note')
            setKeyIconTick((t) => t + 1)
          }}
          title={keyMode === 'camelot' ? 'camelot' : 'Classic'}
          style={{
            background:'transparent',
            border:'none',
            padding:0,
            cursor:'pointer',
            width:25,
            height:25,
            display:'flex',
            alignItems:'center',
            justifyContent:'center'
          }}
        >
          <img
            key={`${keyMode}-${keyIconTick}`}
            className="key-toggle-icon key-toggle-anim"
            src={keyModeIcon}
            alt={tn('切换调性显示', 'Toggle key display')}
            width={25}
            height={25}
            draggable={false}
          />
        </button>
        <button
          onClick={toggleAlwaysOnTop}
          title={alwaysOnTop ? t('pin_title_on') : t('pin_title_off')}
          style={{
            background:'transparent',
            border:'none',
            padding:0,
            cursor:'pointer',
            width:25,
            height:25,
            display:'flex',
            alignItems:'center',
            justifyContent:'center'
          }}
        >
          <img
            src={thumbtack}
            alt={alwaysOnTop ? t('pin_on') : t('pin_title_off')}
            width={25}
            height={25}
            draggable={false}
            style={{
              transform: alwaysOnTop ? 'rotate(-45deg)' : 'none',
              transition:'transform 120ms ease'
            }}
          />
        </button>
        <button
          onClick={async () => {
            try {
              await invoke('enter_floating')
            } catch (e) { }
          }}
          title={t('enter_floating')}
          style={{
            background:'transparent',
            border:'none',
            padding:0,
            cursor:'pointer',
            width:25,
            height:25,
            display:'flex',
            alignItems:'center',
            justifyContent:'center'
          }}
        >
          <img
            src={floatingWindow}
            alt={t('enter_floating')}
            width={25}
            height={25}
            draggable={false}
          />
        </button>
      </div>
      )}

      {/* （已移除更新调试面板） */}
    </main>
  )
}

