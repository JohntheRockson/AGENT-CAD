import { useEffect, useRef, useState } from 'react'
import {
  Send, Bot, User, Brain, PenLine, Box, Cpu, ScanSearch, RotateCcw, ChevronRight, Loader2,
} from 'lucide-react'
import { useCadStore } from '../store/useStore'
import type { ActivityKind, ActivityStep } from '../types/cad'

export function ChatPanel() {
  const messages        = useCadStore((s) => s.messages)
  const isChatLoading   = useCadStore((s) => s.isChatLoading)
  const sendChatMessage = useCadStore((s) => s.sendChatMessage)
  const bodies          = useCadStore((s) => s.bodies)
  const selectedBodyId  = useCadStore((s) => s.selectedBodyId)
  const selectBody      = useCadStore((s) => s.selectBody)

  const selected = bodies.find((b) => b.bodyId === selectedBodyId)

  const [draft, setDraft] = useState('')
  const bottomRef = useRef<HTMLDivElement>(null)
  const taRef     = useRef<HTMLTextAreaElement>(null)

  const resizeInput = (el: HTMLTextAreaElement | null) => {
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(Math.max(el.scrollHeight, 96), 240)}px`
  }

  useEffect(() => {
    resizeInput(taRef.current)
  }, [draft])

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages, isChatLoading])

  const handleSend = () => {
    const text = draft.trim()
    if (!text || isChatLoading) return
    setDraft('')
    sendChatMessage(text)
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border bg-panel">
        <Bot size={15} className="text-accent" />
        <span className="text-xs font-semibold text-gray-200 tracking-wide uppercase">
          Agent
        </span>
        <span className="ml-auto text-[10px] text-accent bg-accent/10 px-1.5 py-0.5 rounded">
          Gemini
        </span>
      </div>

      <div className="flex-1 overflow-y-auto px-5 py-4 space-y-4 min-h-0">
        {messages.length === 0 && (
          <div className="text-sm text-muted text-center mt-10 leading-relaxed">
            Describe the part or assembly you want to build.
            <br />
            <span className="text-xs opacity-60">
              Multi-part designs become separate bodies in the outliner.
            </span>
          </div>
        )}

        {messages.map((msg) => (
          <div
            key={msg.id}
            className={`flex gap-3 ${msg.role === 'user' ? 'flex-row-reverse' : 'flex-row'}`}
          >
            <div
              className={`w-7 h-7 rounded-full flex items-center justify-center flex-shrink-0 ${
                msg.role === 'user' ? 'bg-accent/20' : 'bg-surface border border-border'
              }`}
            >
              {msg.role === 'user' ? (
                <User size={13} className="text-accent" />
              ) : (
                <Bot size={13} className="text-muted" />
              )}
            </div>

            {msg.role === 'user' ? (
              <div className="max-w-[90%] px-3.5 py-2.5 rounded-xl text-sm leading-relaxed whitespace-pre-wrap bg-accent/10 text-gray-200 rounded-tr-sm">
                {msg.content}
              </div>
            ) : (
              <div className="flex-1 min-w-0 max-w-[92%]">
                {msg.steps && msg.steps.length > 0 && (
                  <ActivityLog steps={msg.steps} />
                )}
                {!msg.content && !(msg.steps && msg.steps.length) && isChatLoading && (
                  <div className="flex items-center gap-2 text-xs text-muted py-1">
                    <Loader2 size={12} className="animate-spin text-accent" />
                    Starting…
                  </div>
                )}
                {msg.content ? (
                  <div className="mt-2 px-3.5 py-2.5 rounded-xl text-sm leading-relaxed whitespace-pre-wrap bg-surface border border-border text-gray-300 rounded-tl-sm">
                    {msg.content}
                  </div>
                ) : null}
              </div>
            )}
          </div>
        ))}

        <div ref={bottomRef} />
      </div>

      <div className="border-t border-border p-3">
        {selected && (
          <div className="flex items-center gap-2 mb-2 px-1">
            <span className="text-[10px] text-muted">Editing</span>
            <span className="text-[10px] text-accent bg-accent/10 px-1.5 py-0.5 rounded truncate max-w-[70%]">
              {selected.name}
            </span>
            <button
              type="button"
              onClick={() => selectBody(null)}
              className="ml-auto text-[10px] text-muted hover:text-gray-200"
            >
              whole document
            </button>
          </div>
        )}
        <div className="flex gap-2 items-end">
          <textarea
            ref={taRef}
            className="flex-1 bg-surface border border-border rounded-lg px-3 py-2.5 text-sm
                       text-gray-200 placeholder-muted resize-none focus:outline-none
                       focus:border-accent/50 transition-colors leading-relaxed"
            placeholder="Describe your part… (Enter to send, Shift+Enter for a new line)"
            rows={4}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault()
                handleSend()
              }
            }}
          />
          <button
            onClick={handleSend}
            disabled={!draft.trim() || isChatLoading}
            className="p-2.5 rounded-lg bg-accent/20 text-accent hover:bg-accent/30 disabled:opacity-30
                       disabled:cursor-not-allowed transition-colors flex-shrink-0"
          >
            <Send size={16} />
          </button>
        </div>
      </div>
    </div>
  )
}

function formatMs(ms: number) {
  if (ms < 1000) return `${Math.round(ms)}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

function LiveElapsed({ startedAt }: { startedAt: number }) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 100)
    return () => window.clearInterval(id)
  }, [])
  return <span>{formatMs(Math.max(0, now - startedAt))}</span>
}

const KIND_META: Record<
  ActivityKind,
  { icon: typeof Brain; running: string; done: (ms?: number) => string }
> = {
  thinking: {
    icon: Brain,
    running: 'Thinking',
    done: (ms) => (ms != null ? `Thought for ${formatMs(ms)}` : 'Thought'),
  },
  writing: {
    icon: PenLine,
    running: 'Writing',
    done: (ms) => (ms != null ? `Wrote in ${formatMs(ms)}` : 'Wrote'),
  },
  calculating: {
    icon: Cpu,
    running: 'Calculating',
    done: (ms) => (ms != null ? `Calculated in ${formatMs(ms)}` : 'Calculated'),
  },
  verifying: {
    icon: ScanSearch,
    running: 'Checking result',
    done: (ms) => (ms != null ? `Checked in ${formatMs(ms)}` : 'Checked'),
  },
  rendering: {
    icon: Box,
    running: 'Rendering',
    done: (ms) => (ms != null ? `Rendered in ${formatMs(ms)}` : 'Rendered'),
  },
  repair: {
    icon: RotateCcw,
    running: 'Retrying',
    done: () => 'Retrying',
  },
}

function ActivityLog({ steps }: { steps: ActivityStep[] }) {
  return (
    <div className="space-y-1 py-0.5">
      {steps.map((step) => (
        <ActivityRow key={step.id} step={step} />
      ))}
    </div>
  )
}

function ActivityRow({ step }: { step: ActivityStep }) {
  const meta = KIND_META[step.kind]
  const Icon = meta.icon
  const hasThoughts = step.kind === 'thinking' && !!step.detail?.trim()
  const [open, setOpen] = useState(step.status === 'running')

  useEffect(() => {
    if (step.status === 'running' && hasThoughts) setOpen(true)
  }, [step.status, hasThoughts])

  const label =
    step.status === 'running'
      ? meta.running
      : step.kind === 'repair'
        ? `Retry — ${step.detail ?? 'repairing'}`
        : meta.done(step.ms)

  const row = (
    <div className="flex items-center gap-2 text-xs text-muted min-w-0">
      {step.status === 'running' ? (
        <Loader2 size={12} className="animate-spin text-accent flex-shrink-0" />
      ) : (
        <Icon size={12} className="flex-shrink-0 opacity-70" />
      )}
      <span className="truncate">{label}</span>
      {step.status === 'running' && (
        <span className="tabular-nums text-accent/80">
          <LiveElapsed startedAt={step.startedAt} />
        </span>
      )}
      {hasThoughts && (
        <ChevronRight
          size={12}
          className={`ml-auto flex-shrink-0 opacity-50 transition-transform ${open ? 'rotate-90' : ''}`}
        />
      )}
    </div>
  )

  if (!hasThoughts) return row

  return (
    <div>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="w-full text-left hover:text-gray-300"
      >
        {row}
      </button>
      {open && (
        <pre className="mt-1 mb-1 ml-5 whitespace-pre-wrap text-[11px] leading-relaxed text-muted/90 font-sans border-l border-border pl-2.5 max-h-48 overflow-y-auto">
          {step.detail}
        </pre>
      )}
    </div>
  )
}
