import React from 'react'
import { tn } from '../i18n'

type CopyItemProps = {
  text: string
  label: string
}

export default function CopyItem({ text, label }: CopyItemProps) {
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
