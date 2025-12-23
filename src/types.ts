export type DisplayBpm = { bpm: number, confidence: number, state: 'tracking'|'uncertain'|'analyzing', level: number }
export type DisplayKey = { key: string, camelot: string, confidence: number, state: 'tracking'|'uncertain'|'analyzing'|'atonal' }
export type AudioViz = { samples: number[], rms: number }
