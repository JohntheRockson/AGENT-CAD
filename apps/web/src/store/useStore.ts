import { create } from 'zustand'
import type {
  ActivityKind,
  ActivityStep,
  BodyInstance,
  CadDocument,
  ChatMessage,
  ChatStreamEvent,
  DocumentSnapshot,
  ExportFormat,
  MeshData,
  MetricsData,
  TimelineSource,
} from '../types/cad'
import { runProgram, exportModel, streamChat } from '../lib/api'
import { EXPORT_KINDS, pickSaveTarget, writeSaveTarget } from '../lib/saveFile'
import {
  parseScene,
  parseSceneJson,
  prettyDocument,
  setDocumentParameter,
} from '../lib/document'
import { makeSnapshot, truncateTimelineLabel } from '../lib/timeline'

const EMPTY_IR = ''

function currentDocument(irCode: string): CadDocument | null {
  if (!irCode.trim()) return null
  try {
    return parseSceneJson(irCode)
  } catch {
    return null
  }
}

function applyRunPayload(
  set: (partial: Partial<CadStore> | ((s: CadStore) => Partial<CadStore>)) => void,
  resp: { mesh?: MeshData; metrics?: MetricsData | null; bodies?: BodyInstance[]; error?: string },
  extra: Partial<CadStore> = {},
) {
  const bodies = resp.bodies ?? []
  set((s) => {
    const keep = extra.selectedBodyId !== undefined
      ? extra.selectedBodyId
      : s.selectedBodyId
    return {
      ...extra,
      bodies,
      meshData: resp.mesh ?? bodies.find((b) => b.visible)?.mesh ?? null,
      metrics:  resp.metrics ?? null,
      runError: extra.runError ?? null,
      selectedBodyId: keep && bodies.some((b) => b.bodyId === keep) ? keep : null,
      isolatedBodyId:
        s.isolatedBodyId && bodies.some((b) => b.bodyId === s.isolatedBodyId)
          ? s.isolatedBodyId
          : null,
    }
  })
}

// ── Store interface ───────────────────────────────────────────────────────────

interface CadStore {
  irCode: string
  setIrCode: (code: string) => void

  meshData:   MeshData | null
  metrics:    MetricsData | null
  bodies:     BodyInstance[]
  selectedBodyId: string | null
  hoveredBodyId: string | null
  isolatedBodyId: string | null
  outlinerOpen: boolean
  isRunning:  boolean
  isExporting: boolean
  exportStatus: string | null
  runError:   string | null

  messages:      ChatMessage[]
  isChatLoading: boolean

  timeline:      DocumentSnapshot[]
  timelineIndex: number

  showJson: boolean
  setShowJson: (show: boolean) => void

  runGeometry:     (opts?: RunGeometryOptions) => Promise<void>
  setParameter:    (name: string, value: number) => Promise<void>
  pushTimelineSnapshot: (label: string, source: TimelineSource) => void
  restoreTimelineIndex: (index: number) => void
  branchTimeline:  () => void
  atTimelineTip:   () => boolean
  downloadExport:  (format: ExportFormat) => Promise<void>
  sendChatMessage: (text: string) => Promise<void>
  clearError:      () => void

  selectBody: (id: string | null) => void
  hoverBody: (id: string | null) => void
  setOutlinerOpen: (open: boolean) => void
  setBodyVisible: (id: string, visible: boolean) => void
  isolateBody: (id: string | null) => void
  renameBody: (id: string, name: string) => void
  deleteBody: (id: string) => void
}

interface RunGeometryOptions {
  label?: string
  source?: TimelineSource
  /** Skip branching (caller already branched). */
  skipBranch?: boolean
  /** Skip recording a timeline snapshot after success. */
  skipSnapshot?: boolean
}

// ── Zustand store ─────────────────────────────────────────────────────────────

export const useCadStore = create<CadStore>((set, get) => ({
  irCode:        EMPTY_IR,
  setIrCode:     (code) => set({ irCode: code }),

  meshData:   null,
  metrics:    null,
  bodies:     [],
  selectedBodyId: null,
  hoveredBodyId: null,
  isolatedBodyId: null,
  outlinerOpen: true,
  isRunning:  false,
  isExporting: false,
  exportStatus: null,
  runError:   null,

  messages:      [],
  isChatLoading: false,

  timeline:      [],
  timelineIndex: -1,

  showJson: false,
  setShowJson: (show) => set({ showJson: show }),

  atTimelineTip: () => {
    const { timeline, timelineIndex } = get()
    return timeline.length === 0 || timelineIndex >= timeline.length - 1
  },

  branchTimeline: () => {
    const { timelineIndex, timeline } = get()
    if (timelineIndex < 0 || timelineIndex >= timeline.length - 1) return
    set({ timeline: timeline.slice(0, timelineIndex + 1) })
  },

  pushTimelineSnapshot: (label, source) => {
    const s = get()
    if (!s.irCode.trim()) return
    const snap = makeSnapshot(label, source, {
      irCode:   s.irCode,
      bodies:   s.bodies,
      meshData: s.meshData,
      metrics:  s.metrics,
    })
    set((state) => {
      const base =
        state.timelineIndex >= 0
          ? state.timeline.slice(0, state.timelineIndex + 1)
          : []
      const next = [...base, snap]
      return { timeline: next, timelineIndex: next.length - 1 }
    })
  },

  restoreTimelineIndex: (index) => {
    const snap = get().timeline[index]
    if (!snap) return
    set({
      timelineIndex: index,
      irCode:        snap.irCode,
      bodies:        snap.bodies,
      meshData:      snap.meshData,
      metrics:       snap.metrics,
      runError:      null,
    })
  },

  setParameter: async (name, value) => {
    const doc = currentDocument(get().irCode)
    if (!doc) return
    const current = doc.parameters?.[name]
    if (current != null && Math.abs(current - value) < 1e-9) return
    get().branchTimeline()
    const updated = setDocumentParameter(doc, name, value)
    set({ irCode: prettyDocument(updated) })
    await get().runGeometry({
      label:      `${name} → ${value}`,
      source:     'parameter',
      skipBranch: true,
    })
  },

  clearError: () => set({ runError: null }),

  selectBody: (id) => set({ selectedBodyId: id }),
  hoverBody: (id) => set({ hoveredBodyId: id }),
  setOutlinerOpen: (open) => set({ outlinerOpen: open }),

  setBodyVisible: (id, visible) => {
    const doc = currentDocument(get().irCode)
    if (!doc) return
    doc.bodies = doc.bodies.map((b) => (b.bodyId === id ? { ...b, visible } : b))
    set({
      irCode: prettyDocument(doc),
      bodies: get().bodies.map((b) => (b.bodyId === id ? { ...b, visible } : b)),
    })
  },

  isolateBody: (id) => {
    set({ isolatedBodyId: get().isolatedBodyId === id ? null : id })
  },

  renameBody: (id, name) => {
    const doc = currentDocument(get().irCode)
    if (!doc) return
    doc.bodies = doc.bodies.map((b) => (b.bodyId === id ? { ...b, name } : b))
    set({
      irCode: prettyDocument(doc),
      bodies: get().bodies.map((b) => (b.bodyId === id ? { ...b, name } : b)),
    })
  },

  deleteBody: (id) => {
    const doc = currentDocument(get().irCode)
    if (!doc) return
    doc.bodies = doc.bodies.filter((b) => b.bodyId !== id)
    const bodies = get().bodies.filter((b) => b.bodyId !== id)
    set({
      irCode: doc.bodies.length ? prettyDocument(doc) : '',
      bodies,
      meshData: bodies.find((b) => b.visible)?.mesh ?? null,
      selectedBodyId: get().selectedBodyId === id ? null : get().selectedBodyId,
      isolatedBodyId: get().isolatedBodyId === id ? null : get().isolatedBodyId,
    })
  },

  // ── Run geometry ────────────────────────────────────────────────────────────

  runGeometry: async (opts) => {
    const { irCode } = get()
    if (!irCode.trim()) {
      set({ runError: 'No CAD program to run. Describe a part in chat, or paste JSON here.' })
      return
    }
    if (!opts?.skipBranch) {
      get().branchTimeline()
    }
    set({ isRunning: true, runError: null })

    let document: CadDocument
    try {
      document = parseSceneJson(irCode)
    } catch (e) {
      set({
        isRunning: false,
        runError: `JSON parse error: ${e instanceof Error ? e.message : String(e)}`,
      })
      return
    }

    try {
      const resp = await runProgram(document)
      if (resp.success && (resp.bodies?.length || resp.mesh)) {
        applyRunPayload(set, resp, { isRunning: false, irCode: prettyDocument(document) })
        if (!opts?.skipSnapshot) {
          get().pushTimelineSnapshot(
            opts?.label ?? 'Manual rebuild',
            opts?.source ?? 'manual',
          )
        }
      } else {
        set({
          isRunning: false,
          runError:  resp.error ?? 'Unknown kernel error',
        })
      }
    } catch (e) {
      set({
        isRunning: false,
        runError:  e instanceof Error ? e.message : 'Network error',
      })
    }
  },

  // ── Export ──────────────────────────────────────────────────────────────────

  downloadExport: async (format) => {
    const { irCode } = get()
    if (!irCode.trim()) {
      set({ runError: 'Nothing to export. Generate or paste a CAD program first.' })
      return
    }
    let document: CadDocument
    try {
      document = parseSceneJson(irCode)
    } catch {
      set({ runError: 'Fix JSON before exporting' })
      return
    }

    const kind = EXPORT_KINDS.find((k) => k.id === format)
      ?? EXPORT_KINDS.find((k) => k.ext === format)
    if (!kind) {
      set({ runError: `Unknown export format: ${format}` })
      return
    }

    set({
      isExporting: true,
      exportStatus: 'Choose where to save…',
      runError: null,
    })

    try {
      const target = await pickSaveTarget(kind)
      if (target.kind === 'cancelled') {
        set({ isExporting: false, exportStatus: null })
        return
      }

      set({ exportStatus: `Generating ${kind.label}…` })
      const blob = await exportModel(document, kind.id)

      set({ exportStatus: `Writing ${kind.label}…` })
      const result = await writeSaveTarget(target, blob, kind)
      set({
        isExporting: false,
        exportStatus: result === 'saved' ? `Saved ${kind.label}` : null,
      })
      if (result === 'saved') {
        window.setTimeout(() => {
          const s = get()
          if (s.exportStatus?.startsWith('Saved')) {
            set({ exportStatus: null })
          }
        }, 2500)
      }
    } catch (e) {
      set({
        isExporting: false,
        exportStatus: null,
        runError: e instanceof Error ? e.message : 'Export failed',
      })
    }
  },

  // ── Chat ─────────────────────────────────────────────────────────────────────

  sendChatMessage: async (text) => {
    const { messages, irCode, selectedBodyId, timeline, timelineIndex } = get()
    get().branchTimeline()
    const document = currentDocument(irCode)

    const history = messages
      .filter((m) => m.content.trim())
      .map((m) => ({ role: m.role, content: m.content }))

    const userMsg: ChatMessage = {
      id:        crypto.randomUUID(),
      role:      'user',
      content:   text,
      timestamp: Date.now(),
    }
    const assistantId = crypto.randomUUID()
    const assistantMsg: ChatMessage = {
      id:        assistantId,
      role:      'assistant',
      content:   '',
      timestamp: Date.now(),
      steps:     [],
    }
    set({
      messages:      [...messages, userMsg, assistantMsg],
      isChatLoading: true,
    })

    const patchAssistant = (fn: (m: ChatMessage) => ChatMessage) => {
      set((s) => ({
        messages: s.messages.map((m) => (m.id === assistantId ? fn(m) : m)),
      }))
    }

    const upsertStep = (kind: ActivityKind, patch: Partial<ActivityStep>) => {
      patchAssistant((m) => {
        const steps = [...(m.steps ?? [])]
        let idx = -1
        for (let i = steps.length - 1; i >= 0; i--) {
          if (steps[i].kind === kind) {
            idx = i
            break
          }
        }
        const startNew =
          idx < 0 || (patch.status === 'running' && steps[idx].status === 'done')
        if (startNew) {
          steps.push({
            id:        crypto.randomUUID(),
            kind,
            status:    patch.status ?? 'running',
            startedAt: patch.startedAt ?? Date.now(),
            ms:        patch.ms,
            detail:    patch.detail,
          })
        } else {
          steps[idx] = { ...steps[idx], ...patch }
        }
        return { ...m, steps }
      })
    }

    let gotResult = false

    const handleEvent = (ev: ChatStreamEvent) => {
      switch (ev.type) {
        case 'thinking_start':
          upsertStep('thinking', { status: 'running', startedAt: Date.now(), detail: '' })
          break
        case 'thinking_delta':
          patchAssistant((m) => {
            const steps = [...(m.steps ?? [])]
            let idx = -1
            for (let i = steps.length - 1; i >= 0; i--) {
              if (steps[i].kind === 'thinking') {
                idx = i
                break
              }
            }
            if (idx >= 0) {
              steps[idx] = {
                ...steps[idx],
                detail: (steps[idx].detail ?? '') + ev.text,
              }
            }
            return { ...m, steps }
          })
          break
        case 'thinking_done':
          upsertStep('thinking', { status: 'done', ms: ev.ms })
          break
        case 'writing_start':
          upsertStep('writing', { status: 'running', startedAt: Date.now() })
          break
        case 'writing_done':
          upsertStep('writing', { status: 'done', ms: ev.ms })
          break
        case 'repair':
          patchAssistant((m) => ({
            ...m,
            steps: [
              ...(m.steps ?? []),
              {
                id:        crypto.randomUUID(),
                kind:      'repair',
                status:    'done',
                startedAt: Date.now(),
                detail:    ev.error,
              },
            ],
          }))
          break
        case 'calculating_start':
          set({ isRunning: true })
          upsertStep('calculating', { status: 'running', startedAt: Date.now() })
          break
        case 'calculating_done':
          set({ isRunning: false })
          upsertStep('calculating', { status: 'done', ms: ev.ms })
          break
        case 'verifying_start':
          upsertStep('verifying', { status: 'running', startedAt: Date.now() })
          break
        case 'verifying_done': {
          const v = ev.verification
          const failed = v.checks.filter((c) => !c.passed).length
          const summary =
            failed === 0
              ? `All ${v.checks.length} checks passed`
              : `${failed} of ${v.checks.length} check(s) failed`
          upsertStep('verifying', {
            status: 'done',
            ms:     ev.ms,
            checks: v.checks,
            detail: summary,
          })
          break
        }
        case 'result': {
          gotResult = true
          if (ev.success && (ev.program || ev.mesh || ev.bodies?.length)) {
            const raw = ev.program ?? currentDocument(get().irCode)
            let newIrCode = get().irCode
            try {
              if (raw) newIrCode = prettyDocument(parseScene(raw))
            } catch {
              if (raw) newIrCode = JSON.stringify(raw, null, 2)
            }
            const renderStart = performance.now()
            upsertStep('rendering', { status: 'running', startedAt: Date.now() })
            applyRunPayload(set, ev, { irCode: newIrCode })
            get().pushTimelineSnapshot(
              truncateTimelineLabel(`Agent: ${text}`),
              'agent',
            )
            requestAnimationFrame(() => {
              requestAnimationFrame(() => {
                upsertStep('rendering', {
                  status: 'done',
                  ms: Math.max(1, Math.round(performance.now() - renderStart)),
                })
                patchAssistant((m) => ({ ...m, content: ev.message }))
                set({ isChatLoading: false, isRunning: false })
              })
            })
          } else {
            let irPatch: { irCode?: string } = {}
            if (ev.program) {
              try {
                irPatch = { irCode: prettyDocument(parseScene(ev.program)) }
              } catch {
                irPatch = { irCode: JSON.stringify(ev.program, null, 2) }
              }
            }
            patchAssistant((m) => ({
              ...m,
              content: ev.message || ev.error || 'Could not generate a valid model.',
            }))
            set({
              ...irPatch,
              isChatLoading: false,
              isRunning:     false,
              runError:      ev.error ?? 'AI could not generate a valid model.',
            })
          }
          break
        }
      }
    }

    try {
      const step = timeline[timelineIndex]
      await streamChat(text, history, handleEvent, {
        document: document ?? undefined,
        targetBodyId: selectedBodyId,
        timelineStepIndex: step && timelineIndex >= 0 ? timelineIndex : undefined,
        timelineStepLabel: step?.label,
      })
      if (!gotResult) {
        patchAssistant((m) =>
          m.content
            ? m
            : { ...m, content: 'The agent finished without a summary.' },
        )
        set({ isChatLoading: false, isRunning: false })
      }
    } catch (e) {
      patchAssistant((m) => ({
        ...m,
        content: `Error: ${e instanceof Error ? e.message : String(e)}`,
      }))
      set({ isChatLoading: false, isRunning: false })
    }
  },
}))
