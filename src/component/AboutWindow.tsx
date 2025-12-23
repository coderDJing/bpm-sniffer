import React from 'react'
import { t } from '../i18n'
import CopyItem from './CopyItem'

type AboutWindowProps = {
  themeName: 'dark'|'light'
  appVersion: string
}

export default function AboutWindow({ themeName, appVersion }: AboutWindowProps) {
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
