import React, { useEffect, useRef, useState } from 'react'
import WaveViz from './component/WaveViz'
import BarsViz from './component/BarsViz'
import WaterfallViz from './component/WaterfallViz'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import thumbtack from './assets/thumbtack.png'
import sun from './assets/sun.png'
import moon from './assets/moon.png'
import refresh from './assets/refresh.png'
// @ts-ignore: optional plugin at runtime
import { check } from '@tauri-apps/plugin-updater'
import { getVersion } from '@tauri-apps/api/app'
import { t, getCurrentLang, tn } from './i18n'

type DisplayBpm = { bpm: number, confidence: number, state: 'tracking'|'uncertain'|'analyzing', level: number }
type AudioViz = { samples: number[], rms: number }

export default function App() {
  const [route, setRoute] = useState<string>(typeof window !== 'undefined' ? window.location.hash : '')
  const [bpm, setBpm] = useState<number | null>(null)
  const [conf, setConf] = useState<number | null>(null)
  const [state, setState] = useState<DisplayBpm['state']>('analyzing')
  const [alwaysOnTop, setAlwaysOnTop] = useState<boolean>(false)
  const [viz, setViz] = useState<AudioViz | null>(null)
  const [vizMode, setVizMode] = useState<'wave'|'bars'|'waterfall'>('wave')
  const [themeName, setThemeName] = useState<'dark' | 'light'>('dark')
  const [appVersion, setAppVersion] = useState<string>('')
  const [updateReady, setUpdateReady] = useState<boolean>(false)
  // （已移除前端调试面板与快捷键）
  const mqlCleanupRef = useRef<null | (() => void)>(null)
  // 高亮锁：当某个值在高置信度下被高亮后，如果之后收到同值但低置信度的数据，仍保持高亮，直到值发生变化
  const bpmRef = useRef<number | null>(null)
  const highlightLockRef = useRef<{ locked: boolean, bpm: number | null }>({ locked: false, bpm: null })
  // 低置信度同值连续计数：当灰显同值连续达到阈值（如5）时，自动视为需要高亮
  const lowConfStreakRef = useRef<{ bpm: number | null, count: number }>({ bpm: null, count: 0 })
  const lowConfPromoteThreshold = 5

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
        } catch {}

        // 通知后端日志语言（中文/英文）
        try { await invoke('set_log_lang', { is_zh: getCurrentLang() === 'zh-CN' }) } catch {}
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
        const unlistenD = await listen<AudioViz>('viz_update', (e) => {
          setViz(e.payload as any as AudioViz)
        })
        removeListener = () => { if (removeMql) removeMql(); unlistenA(); unlistenD() }
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
  // 当高亮锁生效且锁定的值与当前显示值一致时，始终使用高亮颜色
  const currentDisplayedBpm = (bpm == null ? bpmRef.current : bpm)
  const currentDisplayedIntForColor = currentDisplayedBpm != null ? Math.round(currentDisplayedBpm) : null
  const isLockedHighlight = !!(highlightLockRef.current.locked && highlightLockRef.current.bpm != null && highlightLockRef.current.bpm === currentDisplayedIntForColor)
  const confLabel = isLockedHighlight ? t('conf_stable') : baseConfLabel
  const confColor = isLockedHighlight ? theme.textPrimary : baseConfColor
  const bpmColor = isLockedHighlight ? theme.textPrimary : (conf == null ? theme.confGray : (conf >= 0.5 ? theme.textPrimary : theme.confGray))
  // 状态标签“节拍不稳”在锁定时显示为“追踪中”，但“分析中”优先级最高，始终显示“分析中”
  const label = state === 'analyzing' ? t('state_analyzing') : (isLockedHighlight ? t('state_tracking') : baseLabel)

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
      setHideRms(h < 350)
      setHideViz(h < 320)
      setHideTitle(h < 180)
      setHideMeta(h < 160)
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

  // 独立关于窗口
  if (route === '#about') {
    return (
      <AboutWindow themeName={themeName} setThemeName={setThemeName} appVersion={appVersion} />
    )
  }
  if (route === '#logs') {
    return (
      <LogsWindow themeName={themeName} />
    )
  }

  return (
    <main style={{height:'100vh',display:'flex',flexDirection:'column',alignItems:'center',justifyContent: hideViz ? 'center' : 'flex-start',gap:16,background:theme.background,color:theme.textPrimary,overflow:'hidden'}}>
      {updateReady && (
        <div style={{position:'fixed',left:'50%',transform:'translateX(-50%)',bottom:16,background:theme.panelBg,border:'1px solid #1d2a3a',borderRadius:8,padding:'10px 12px',display:'flex',alignItems:'center',gap:10,zIndex:9999,boxShadow:'0 4px 12px rgba(0,0,0,0.35)',minWidth:'min(360px, calc(100vw - 32px))',maxWidth:'calc(100vw - 32px)',flexWrap:'nowrap',justifyContent:'flex-start'}}>
          <span style={{fontSize:12,color:theme.textPrimary,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis',flex:'1 1 auto',minWidth:0}}>{t('update_ready')}</span>
          <button onClick={() => setUpdateReady(false)} style={{fontSize:12,background:'transparent',border:'1px solid #3a0b17',color:theme.textSecondary,borderRadius:6,cursor:'pointer',padding:'4px 8px',whiteSpace:'nowrap',display:'inline-flex',alignItems:'center',justifyContent:'center'}}>{t('close')}</button>
        </div>
      )}
      <div style={{flex:'1 1 auto', width:'100%', display:'flex', flexDirection:'column', alignItems:'center', justifyContent:'center', gap:0, minHeight: hideViz ? '100vh' : undefined}}>
        {!hideTitle && <h1 style={{margin:0,color:'#eb1a50',fontSize:18}}>{t('app_title')}</h1>}
        <div style={{fontSize:96,fontWeight:700,letterSpacing:2,color:bpmColor,height:'100px',lineHeight:'100px'}}>{bpm == null ? 0 : Math.round(bpm)}</div>
        {!hideMeta && (
          <div style={{fontSize:14,color:theme.textSecondary,paddingTop:'10px'}}>
            {label} · {t('conf_label')}<span style={{color: confColor}}>{confLabel}</span>
          </div>
        )}
      </div>

      {/* 简易波形可视化 */}
      {!hideViz && (
        <div style={{marginTop:'auto', marginBottom:7}}>
          <VizPanel theme={theme} hideRms={hideRms} viz={viz} mode={vizMode} onToggle={() => setVizMode(m => m==='wave' ? 'bars' : (m==='bars' ? 'waterfall' : 'wave'))} />
        </div>
      )}

      {!hideActions && (
      <div style={{position:'fixed',right:12,top:12,display:'flex',gap:8,alignItems:'center'}}>
        <button
          onClick={async () => {
            if (refreshSpin) return
            setRefreshSpin(true)
            try {
              // 清空前端可见状态
              setBpm(null); bpmRef.current = null
              setConf(null)
              setState('analyzing')
              setViz(null)
              highlightLockRef.current = { locked: false, bpm: null }
              lowConfStreakRef.current = { bpm: null, count: 0 }
              // 通知后端软重置
              try { await invoke('reset_backend') } catch {}
            } finally {
              // 启动一次 360° 顺时针旋转动画
              setTimeout(() => setRefreshSpin(false), 420)
            }
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
      </div>
      )}

      {/* （已移除更新调试面板） */}
    </main>
  )
}

function AboutWindow({ themeName, setThemeName, appVersion }: { themeName: 'dark'|'light', setThemeName: (v: 'dark'|'light') => void, appVersion: string }) {
  const darkTheme = {
    background: '#14060a', textPrimary: '#ffffff', textSecondary: '#f3a0b3', panelBg: '#1a0a0f'
  }
  const lightTheme = {
    background: '#fff4f7', textPrimary: '#1b0a10', textSecondary: '#b21642', panelBg: '#ffe8ee'
  }
  const theme = themeName === 'dark' ? darkTheme : lightTheme
  // 仅展示文字地址，不再尝试打开浏览器
  return (
    <main style={{height:'100vh',display:'flex',flexDirection:'column',alignItems:'center',justifyContent:'center',gap:12,background:theme.background,color:theme.textPrimary,overflow:'hidden'}}>
      <div style={{width:320, background:theme.panelBg, border:'1px solid #1d2a3a', borderRadius:8, padding:14}}>
        <div style={{fontWeight:700, marginBottom:8, color:'#eb1a50'}}>{t('about_title')}</div>
        <div style={{fontSize:13, lineHeight:1.6}}>
          <div style={{marginBottom:6}}>BPM Sniffer {appVersion ? `v${appVersion}` : ''}</div>
          {/* 预发布提示：仅当 VITE_RELEASE_CHANNEL === 'pre' 时显示 */}
          {import.meta.env.VITE_RELEASE_CHANNEL === 'pre' && (
            <div style={{margin:'6px 0 10px 0', padding:'6px 8px', border:'1px dashed #b21642', borderRadius:6, color:theme.textSecondary}}>
              <div style={{fontWeight:600, marginBottom:4}}>{t('pre_tip_title')}</div>
              <div style={{fontSize:12}}>{t('pre_tip_text')}</div>
            </div>
          )}
          <div style={{display:'flex', flexDirection:'column', gap:2}}>
            <span style={{color:theme.textPrimary}}>{t('about_project')}</span>
            <CopyItem text="https://github.com/coderDJing/bpm-sniffer" label="https://github.com/coderDJing/bpm-sniffer" />
          </div>
          <div style={{display:'flex', flexDirection:'column', gap:2, marginTop:6}}>
            <span style={{color:theme.textPrimary}}>{t('about_author')}</span>
            <span style={{color:theme.textSecondary}}>{t('about_author_name')}</span>
          </div>
          <div style={{display:'flex', flexDirection:'column', gap:2, marginTop:6}}>
            <span style={{color:theme.textPrimary}}>{t('about_contact')}</span>
            <CopyItem text="jinlingwuyanzu@qq.com" label="jinlingwuyanzu@qq.com" />
          </div>
        </div>
        {/* 关闭按钮已移除，保持简洁展示 */}
      </div>
    </main>
  )
}

function LogsWindow({ themeName }: { themeName: 'dark'|'light' }) {
  const darkTheme = { background: '#14060a', textPrimary: '#ffffff', textSecondary: '#f3a0b3', panelBg: '#1a0a0f' }
  const lightTheme = { background: '#fff4f7', textPrimary: '#1b0a10', textSecondary: '#b21642', panelBg: '#ffe8ee' }
  const theme = themeName === 'dark' ? darkTheme : lightTheme
  const [logs, setLogs] = React.useState<string[]>([])
  const wrapRef = React.useRef<HTMLDivElement | null>(null)
  const atBottomRef = React.useRef(true)

  React.useEffect(() => {
    let unlisten: (() => void) | null = null
    ;(async () => {
      unlisten = await listen<string>('friendly_log', (e) => {
        const msg = String(e.payload || '')
        setLogs((prev) => {
          const last = prev.length ? prev[prev.length - 1] : null
          const dupQuiet = tn('暂未检测到清晰节拍，继续聆听…', 'No clear beat yet. Listening…')
          const dupSilent = tn('检测到环境安静，BPM 为 0（等待声音）', 'Silence detected. BPM is 0 (waiting for audio)')
          if ((msg === dupQuiet || msg === dupSilent) && last === msg) return prev
          return [...prev, msg]
        })
      })
    })()
    return () => { if (unlisten) unlisten() }
  }, [])

  React.useEffect(() => {
    const el = wrapRef.current
    if (!el) return
    const nearBottom = Math.abs(el.scrollHeight - el.clientHeight - el.scrollTop) < 8
    if (nearBottom || atBottomRef.current) {
      el.scrollTop = el.scrollHeight
      atBottomRef.current = true
    }
  }, [logs])

  function onScroll(e: React.UIEvent<HTMLDivElement>) {
    const el = e.currentTarget
    const nearBottom = Math.abs(el.scrollHeight - el.clientHeight - el.scrollTop) < 8
    atBottomRef.current = nearBottom
  }

  const title = tn('分析日志', 'Logs')
  return (
    <main style={{height:'100vh',display:'flex',flexDirection:'column',gap:8,background:theme.background,color:theme.textPrimary,overflow:'hidden'}}>
      <div style={{padding:'10px 10px 0 10px',display:'flex',alignItems:'center',justifyContent:'space-between'}}>
        <div style={{fontWeight:700,color:'#eb1a50'}}>{title}</div>
        <div style={{display:'flex',gap:8}}>
          <button onClick={() => setLogs([])} style={{fontSize:12,background:'transparent',border:'1px solid #3a0b17',color:theme.textSecondary,borderRadius:6,cursor:'pointer',padding:'4px 8px'}}>{tn('清空', 'Clear')}</button>
        </div>
      </div>
      <div className="logs-scroll logs-selectable" ref={wrapRef} onScroll={onScroll} style={{flex:'1 1 auto',margin:'0 10px 10px 10px',background:theme.panelBg,border:'1px solid #1d2a3a',borderRadius:6,overflow:'auto',padding:8}}>
        {logs.length === 0 ? (
          <div style={{fontSize:12,color:theme.textSecondary}}>{tn('打开后开始接收日志…', 'Receiving logs since opened…')}</div>
        ) : (
          <pre style={{margin:0,whiteSpace:'pre-wrap',wordBreak:'break-word',fontSize:12,lineHeight:1.5}}>
            {logs.map((l, i) => (
              <div key={i}>{l}</div>
            ))}
          </pre>
        )}
      </div>
    </main>
  )
}

function CopyItem({ text, label }: { text: string, label: string }) {
  const [tip, setTip] = React.useState<string | null>(null)
  async function onCopy() {
    try {
      await navigator.clipboard.writeText(text)
      setTip(tn('已复制到剪贴板', 'Copied to clipboard'))
      setTimeout(() => setTip(null), 1200)
    } catch {}
  }
  return (
    <div style={{position:'relative'}}>
      <button onClick={onCopy} style={{textAlign:'left',background:'transparent',border:'none',padding:0,cursor:'pointer',color:'#b21642'}}>{label}</button>
      {tip && (
        <div style={{position:'absolute', left:0, top:-24, background:'rgba(0,0,0,0.75)', color:'#fff', borderRadius:6, padding:'2px 6px', fontSize:11}}>
          {tip}
        </div>
      )}
    </div>
  )
}

function VizPanel({ theme, hideRms, viz, mode, onToggle }: { theme: any, hideRms: boolean, viz: AudioViz | null, mode: 'wave'|'bars'|'waterfall', onToggle: () => void }) {
  // 自适应高度：在默认窗口高度（≈390）时保持 120px，随着窗口拉高按比例增大，设上下限
  const baseWindowH = 390
  const baseVizH = 120
  const vh = typeof window !== 'undefined' ? window.innerHeight : baseWindowH
  const h = Math.max(100, Math.min(300, Math.floor(baseVizH + Math.max(0, vh - baseWindowH) * 0.7)))
  const w = Math.max(180, Math.floor(window.innerWidth - 10))
  const bg = theme.panelBg
  const grid = theme.grid
  const line = theme.line
  const accent = theme.accent
  const rmsRaw = viz?.rms ?? 0
  const samples = viz?.samples ?? []
  const silentCut = 0.015
  // 快速静音判定：当音量相对上一帧骤降，直接视为静音（解决“戛然而止”不归零）
  const prevRmsRef = React.useRef(0)
  const [fastSilent, setFastSilent] = React.useState(false)
  React.useEffect(() => {
    const prev = prevRmsRef.current
    const drop = prev > 0.08 && rmsRaw < prev * 0.25
    setFastSilent(drop)
    prevRmsRef.current = rmsRaw
  }, [rmsRaw])
  const isSilent = rmsRaw < silentCut || fastSilent

  // 帧间平滑：用 ref 避免 setState 导致的渲染环
  const lastSmoothedRef = React.useRef<number[] | null>(null)
  const smoothSamples = React.useMemo(() => {
    const alpha = 0.35
    const base = isSilent ? new Array(samples.length).fill(0) : samples
    if (!base.length) return [] as number[]
    const prev = lastSmoothedRef.current
    if (!prev || prev.length !== base.length) {
      lastSmoothedRef.current = base
      return base
    }
    const out = new Array(base.length)
    for (let i = 0; i < base.length; i++) {
      out[i] = prev[i] * (1 - alpha) + base[i] * alpha
    }
    lastSmoothedRef.current = out
    return out
  }, [samples, isSilent])

  // RMS 平滑（用 ref，避免触发重渲染环）
  const rmsSmoothedRef = React.useRef(0)
  React.useEffect(() => {
    const prev = rmsSmoothedRef.current
    if (isSilent) { rmsSmoothedRef.current = 0; return }
    const alphaUp = 0.15
    const alphaDown = 0.35
    const alpha = rmsRaw < prev ? alphaDown : alphaUp
    rmsSmoothedRef.current = prev * (1 - alpha) + rmsRaw * alpha
  }, [rmsRaw, isSilent])

  // 自适应增益（用 ref）
  const peakRef = React.useRef(0.3)
  React.useEffect(() => {
    if (!smoothSamples.length) return
    let localPeak = 0
    for (let i = 0; i < smoothSamples.length; i++) {
      const a = Math.abs(smoothSamples[i] || 0)
      if (a > localPeak) localPeak = a
    }
    peakRef.current = isSilent ? 0.2 : (peakRef.current * 0.95 + localPeak * 0.05)
  }, [smoothSamples, isSilent])
  // 降低基础增益，并限制上下限，取折中视觉效果
  const base = Math.max(0.12, Math.min(0.6, peakRef.current))
  let gain = 0.6 / base
  gain = Math.max(0.8, Math.min(2.2, gain))

  // Waterfall：把每帧样本折叠成 bands 段能量（历史用 ref）
  const histRef = React.useRef<number[][]>([])
  const bands = 16
  const scrollMul = 6
  const gap = 1
  // 目标：在高度变化时保持与默认高度时一致的上下留白，同时自适应填充可用空间
  const baseCell = 4
  const baseH = baseVizH
  const baseContentH = Math.min(baseH, bands * (baseCell + gap) + gap)
  const idealPaddingY = Math.max(0, Math.floor((baseH - baseContentH) / 2))
  const targetContentH = Math.max(10, h - idealPaddingY * 2)
  const cell = Math.max(2, Math.min(16, Math.floor((targetContentH - gap) / bands - gap)))
  const visibleCols = Math.max(1, Math.floor((w - gap) / (cell + gap)))
  const wfW = Math.min(w, visibleCols * (cell + gap) + gap)
  const wfH = Math.min(h, bands * (cell + gap) + gap)
  const wfOffsetX = Math.max(0, Math.floor((w - wfW) / 2))
  // 以默认上下留白 idealPaddingY 为基线，尽量把因取整产生的剩余高度对称分配到上下
  const extraY = Math.max(0, Math.floor((h - 2 * idealPaddingY - wfH) / 2))
  const wfOffsetY = Math.max(0, idealPaddingY + extraY)
  const maxHistory = 600
  // 将一帧样本折叠为 bands 段能量
  const frameBands = React.useMemo(() => {
    const out = new Array(bands).fill(0)
    if (!smoothSamples.length) return out
    const step = Math.max(1, Math.floor(smoothSamples.length / bands))
    for (let b = 0; b < bands; b++) {
      const i0 = b * step
      const i1 = Math.min(smoothSamples.length, i0 + step)
      let acc = 0, cnt = 0
      for (let i = i0; i < i1; i++) { acc += Math.abs(smoothSamples[i] || 0); cnt++ }
      const v = cnt ? acc / cnt : 0
      out[b] = Math.min(1, v * gain)
    }
    return out
  }, [smoothSamples, gain])
  // 维护历史（右侧为最新），按 scrollMul 倍速推进（不变稀疏：仅推进历史，不减少可见列）
  React.useEffect(() => {
    if (!frameBands.length) return
    let next = histRef.current.slice()
    for (let i = 0; i < scrollMul; i++) {
      next.push(frameBands)
    }
    if (next.length > maxHistory) next = next.slice(next.length - maxHistory)
    histRef.current = next
  }, [frameBands])
  function heatColor(v: number) {
    // 强化对比度但保持原色系：
    // 低 -> #24060d, 中(≈#eb1a50) -> 高 -> #ffd6e1
    const clamp = (x: number) => Math.max(0, Math.min(1, x))
    const enhance = (x: number, c = 1.45, pivot = 0.55) => clamp((x - pivot) * c + pivot)
    const t = enhance(clamp(v))
    const mid = 0.6
    const lerp = (a: number, b: number, t: number) => Math.round(a + (b - a) * t)
    if (t <= mid) {
      const u = t / mid
      const r = lerp(36, 235, u) // 24 -> eb
      const g = lerp(6, 26, u)   // 06 -> 1a
      const b = lerp(13, 80, u)  // 0d -> 50
      return `rgb(${r},${g},${b})`
    } else {
      const u = (t - mid) / (1 - mid)
      const r = lerp(235, 255, u)
      const g = lerp(26, 214, u)
      const b = lerp(80, 225, u)
      return `rgb(${r},${g},${b})`
    }
  }
  const barLen = Math.round(rmsSmoothedRef.current * (w - 60))
  return (
    <div style={{width:w, display:'flex', flexDirection:'column', gap:6, padding:'0 5px'}}>
      <svg width={w-10} height={h} style={{background:bg, border:'1px solid #1d2a3a', borderRadius:6, cursor:'pointer'}} onClick={onToggle}>
        {mode === 'wave' && (
          <WaveViz width={w-10} height={h} samples={smoothSamples} gain={gain} gridColor={grid} lineColor={line} />
        )}
        {mode === 'bars' && (
          <BarsViz width={w-10} height={h} samples={smoothSamples} gain={gain} barColor={line} />
        )}
        {mode === 'waterfall' && (
          <WaterfallViz width={w-10} height={h} bands={bands} gap={gap} cell={cell} history={histRef.current} heatColor={heatColor} overrideOffsetY={wfOffsetY} />
        )}
      </svg>
      {!hideRms && (
        <div title={t('rms_tooltip')} style={{display:'flex', alignItems:'center', gap:8, height:14}}>
          <div style={{width:Math.max(60, w-70), height:8, background:'#3a0b17', borderRadius:4, overflow:'hidden'}}>
            <div style={{width:barLen, height:'100%', background:accent, transition:'width 120ms'}} />
          </div>
          <span style={{fontSize:12, lineHeight:'14px', color:'#6b829e'}}>
            RMS {Math.round(rmsSmoothedRef.current*100)}%
          </span>
        </div>
      )}
    </div>
  )
}
