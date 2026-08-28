import type { CadDocument, CadProgram, ChatStreamEvent, ExportFormat, RunResponse } from '../types/cad'

const BASE = '/api'

export async function runProgram(program: CadProgram | CadDocument): Promise<RunResponse> {
  const res = await fetch(`${BASE}/run`, {
    method:  'POST',
    headers: { 'Content-Type': 'application/json' },
    body:    JSON.stringify(
      'bodies' in program ? { document: program } : { program },
    ),
  })
  if (!res.ok) {
    const text = await res.text().catch(() => 'Unknown error')
    throw new Error(`Server ${res.status}: ${text}`)
  }
  return res.json() as Promise<RunResponse>
}

export async function listTopology(
  program: CadProgram | CadDocument,
): Promise<{ success: boolean; topology?: unknown; error?: string }> {
  const res = await fetch(`${BASE}/topology`, {
    method:  'POST',
    headers: { 'Content-Type': 'application/json' },
    body:    JSON.stringify(
      'bodies' in program ? { document: program } : { program },
    ),
  })
  if (!res.ok) {
    const text = await res.text().catch(() => 'Unknown error')
    throw new Error(`Topology ${res.status}: ${text}`)
  }
  return res.json()
}

export async function exportModel(
  program: CadProgram | CadDocument,
  format: ExportFormat,
): Promise<Blob> {
  const res = await fetch(`${BASE}/export`, {
    method:  'POST',
    headers: { 'Content-Type': 'application/json' },
    body:    JSON.stringify(
      'bodies' in program
        ? { document: program, format }
        : { program, format },
    ),
  })
  if (!res.ok) {
    const text = await res.text().catch(() => 'Unknown error')
    throw new Error(`Export failed ${res.status}: ${text}`)
  }
  return res.blob()
}

export async function streamChat(
  message: string,
  history: Array<{ role: string; content: string }>,
  onEvent: (ev: ChatStreamEvent) => void,
  extras?: { document?: CadDocument; targetBodyId?: string | null },
): Promise<void> {
  const res = await fetch(`${BASE}/chat`, {
    method:  'POST',
    headers: {
      'Content-Type': 'application/json',
      Accept: 'text/event-stream',
    },
    body: JSON.stringify({
      message,
      history,
      document: extras?.document,
      targetBodyId: extras?.targetBodyId || undefined,
    }),
  })
  if (!res.ok || !res.body) {
    const text = await res.text().catch(() => 'Unknown error')
    throw new Error(`Chat API ${res.status}: ${text}`)
  }

  const reader  = res.body.getReader()
  const decoder = new TextDecoder()
  let buf = ''

  while (true) {
    const { done, value } = await reader.read()
    if (done) break
    buf += decoder.decode(value, { stream: true })

    buf = buf.replace(/\r\n/g, '\n')
    while (true) {
      const idx = buf.indexOf('\n\n')
      if (idx < 0) break
      const raw = buf.slice(0, idx)
      buf = buf.slice(idx + 2)
      for (const line of raw.split('\n')) {
        const trimmed = line.trim()
        if (!trimmed.startsWith('data:')) continue
        const data = trimmed.slice(5).trim()
        if (!data || data === '[DONE]') continue
        try {
          onEvent(JSON.parse(data) as ChatStreamEvent)
        } catch {
          // ignore keep-alive / malformed chunks
        }
      }
    }
  }
}

export async function healthCheck(): Promise<boolean> {
  try {
    const res = await fetch(`${BASE}/health`)
    return res.ok
  } catch {
    return false
  }
}
