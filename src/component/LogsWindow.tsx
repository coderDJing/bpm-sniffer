import React from 'react'
import { listen } from '@tauri-apps/api/event'
import { tn } from '../i18n'

type LogsWindowProps = {
  themeName: 'dark'|'light'
}

export default function LogsWindow({ themeName }: LogsWindowProps) {
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
