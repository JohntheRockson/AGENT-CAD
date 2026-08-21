import { useEffect, useRef, useState } from 'react'
import {
  Send, Bot, User, Brain, PenLine, Box, Cpu, ScanSearch,
  RotateCcw, ChevronRight, Loader2, Copy, Check, Sparkles, X,
} from 'lucide-react'
import { useCadStore } from '../store/useStore'
import type { ActivityKind, ActivityStep } from '../types/cad'

// ── Suggestion chips ──────────────────────────────────────────────────

const SUGGESTIONS = [
  'Design a 60mm mounting bracket with 4 bolt holes',
  'Create a spur gear, 20 teeth, module 2',
  'Make an enclosure for a PCB, 80×60×25mm',
  'Build a threaded M8 bolt, 40mm long',
  'Design a pipe elbow, DN25, 90° bend',
  'Create a hex standoff, M3, 10mm',
]

// ── Main component ────────────────────────────────────────────────────

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
    el.style.height = `${Math.min(Math.max(el.scrollHeight, 72), 220)}px`
  }

  useEffect(() => { resizeInput(taRef.current) }, [draft])
  useEffect(() => { bottomRef.current?.scrollIntoView({ behavior: 'smooth' }) }, [messages, isChatLoading])

  const handleSend = () => {
    const text = draft.trim()
    if (!text || isChatLoading) return
    setDraft('')
    sendChatMessage(text)
  }

  return (
    <div className="flex flex-col h-full min-h-0">

      {/* ── Header ─────────────────────────────────────────────────── */}
      <div className="flex items-center gap-2 px-3 h-9 border-b border-border flex-shrink-0">
        <div className="w-5 h-5 rounded bg-accent/20 border border-accent/30 flex items-center justify-center">
          <Sparkles size={11} className="text-accent" />
        </div>
        <span className="text-[11px] font-semibold text-gray-300 tracking-wide uppercase flex-1">
          AI Assistant
        </span>
        <span className="text-[9px] text-accent bg-accent/10 border border-accent/20 px-1.5 py-0.5 rounded font-mono">
          Gemini
        </span>
        {isChatLoading && (
          <Loader2 size={11} className="animate-spin text-accent" />
        )}
      </div>

      {/* ── Message list ───────────────────────────────────────────── */}
      <div className="flex-1 overflow-y-auto px-3 py-3 space-y-4 min-h-0">

        {/* Empty state with suggestions */}
        {messages.length === 0 && (
          <div className="space-y-4">
            <div className="text-center pt-4">
              <div className="w-10 h-10 rounded-full bg-accent/10 border border-accent/20 flex items-center justify-center mx-auto mb-3">
                <Bot size={20} className="text-accent" />
              </div>
              <p className="text-[12px] text-gray-300 font-medium">AgentCAD AI</p>
              <p className="text-[11px] text-muted mt-1 leading-relaxed">
                Describe the part or assembly you want to build.
                Multi-body designs become separate bodies in the model tree.
              </p>
            </div>

            <div className="space-y-1.5">
              <p className="text-[9px] text-dim font-semibold uppercase tracking-widest px-1">
                Try a prompt
              </p>
              {SUGGESTIONS.map((s) => (
                <button
                  key={s}
                  type="button"
                  onClick={() => { setDraft(s); taRef.current?.focus() }}
                  className="w-full text-left px-3 py-2 rounded-lg bg-raised border border-divide
                             text-[11px] text-muted hover:text-gray-200 hover:border-accent/30
                             hover:bg-accent/5 transition-all"
                >
                  {s}
                </button>
              ))}
            </div>
          </div>
        )}

        {/* Messages */}
        {messages.map((msg) => (
          <div
            key={msg.id}
            className={`flex gap-2.5 ${msg.role === 'user' ? 'flex-row-reverse' : 'flex-row'}`}
          >
            {/* Avatar */}
            <div
              className={`w-6 h-6 rounded-full flex items-center justify-center flex-shrink-0 mt-0.5
                          ${msg.role === 'user'
                            ? 'bg-accent/20 border border-accent/30'
                            : 'bg-surface border border-border'
                          }`}
            >
              {msg.role === 'user'
                ? <User size={11} className="text-accent" />
                : <Bot  size={11} className="text-muted" />
              }
            </div>

            {/* Content */}
            {msg.role === 'user' ? (
              <div className="max-w-[88%] min-w-0">
                <div className="px-3 py-2 rounded-xl rounded-tr-sm bg-accent/12 border border-accent/15
                                text-[12px] leading-relaxed text-gray-200 whitespace-pre-wrap select-text cursor-text">
                  {msg.content}
                </div>
                <CopyLine text={msg.content} />
              </div>
            ) : (
              <div className="flex-1 min-w-0 max-w-[92%] space-y-1.5">
                {msg.steps && msg.steps.length > 0 && (
                  <ActivityLog steps={msg.steps} />
                )}
                {!msg.content && !(msg.steps?.length) && isChatLoading && (
                  <div className="flex items-center gap-1.5 text-[11px] text-muted py-1">
                    <Loader2 size={11} className="animate-spin text-accent" />
                    Starting…
                  </div>
                )}
                {msg.content && (
                  <div>
                    <div className="px-3 py-2 rounded-xl rounded-tl-sm bg-raised border border-border
                                    text-[12px] leading-relaxed text-gray-300 whitespace-pre-wrap
                                    select-text cursor-text">
                      {msg.content}
                    </div>
                    <CopyLine text={msg.content} />
                  </div>
                )}
              </div>
            )}
          </div>
        ))}

        <div ref={bottomRef} />
      </div>

      {/* ── Input area ─────────────────────────────────────────────── */}
      <div className="border-t border-border p-2.5 space-y-2 flex-shrink-0">

        {/* Body scope chip */}
        {selected && (
          <div className="flex items-center gap-1.5 px-1">
            <span className="text-[10px] text-dim">Editing body:</span>
            <span className="text-[10px] text-accent bg-accent/10 border border-accent/20 px-1.5 py-0.5 rounded truncate max-w-[60%]">
              {selected.name}
            </span>
            <button
              type="button"
              onClick={() => selectBody(null)}
              className="ml-auto text-dim hover:text-muted transition-colors"
              title="Edit whole document"
            >
              <X size={11} />
            </button>
          </div>
        )}

        {/* Textarea + send */}
        <div className="flex gap-2 items-end">
          <div className="flex-1 relative">
            <textarea
              ref={taRef}
              className="w-full bg-surface border border-border rounded-xl px-3 py-2.5 pr-2 text-[12px]
                         text-gray-200 placeholder-dim resize-none focus:outline-none
                         focus:border-accent/50 transition-colors leading-relaxed"
              placeholder="Describe your part… (Enter to send)"
              rows={3}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleSend() }
              }}
            />
            {draft && (
              <span className="absolute bottom-2 right-2 text-[9px] text-dim font-mono">
                {draft.length}
              </span>
            )}
          </div>
          <button
            onClick={handleSend}
            disabled={!draft.trim() || isChatLoading}
            className="w-9 h-9 rounded-xl bg-accent text-white flex items-center justify-center
                       hover:bg-accent-lite disabled:opacity-30 disabled:cursor-not-allowed
                       transition-colors flex-shrink-0 shadow-sm"
          >
            <Send size={14} />
          </button>
        </div>

        {/* Hint */}
        <p className="text-[10px] text-dim px-1">
          <kbd className="text-[9px] bg-raised border border-divide rounded px-1 py-0.5">Enter</kbd> send
          &nbsp;·&nbsp;
          <kbd className="text-[9px] bg-raised border border-divide rounded px-1 py-0.5">Shift+Enter</kbd> new line
        </p>
      </div>
    </div>
  )
}

// ── Copy button ───────────────────────────────────────────────────────

function CopyLine({ text }: { text: string }) {
  const [copied, setCopied] = useState(false)
  if (!text.trim()) return null
  return (
    <button
      type="button"
      className="mt-1 inline-flex items-center gap-1 text-[10px] text-dim hover:text-muted transition-colors"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text)
          setCopied(true)
          window.setTimeout(() => setCopied(false), 1500)
        } catch { /* ignore */ }
      }}
    >
      {copied ? <Check size={9} /> : <Copy size={9} />}
      {copied ? 'Copied' : 'Copy'}
    </button>
  )
}

// ── Activity log ──────────────────────────────────────────────────────

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
  return <span className="tabular-nums">{formatMs(Math.max(0, now - startedAt))}</span>
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
    running: 'Calculating geometry',
    done: (ms) => (ms != null ? `Calculated in ${formatMs(ms)}` : 'Calculated'),
  },
  verifying: {
    icon: ScanSearch,
    running: 'Verifying result',
    done: (ms) => (ms != null ? `Verified in ${formatMs(ms)}` : 'Verified'),
  },
  rendering: {
    icon: Box,
    running: 'Rendering mesh',
    done: (ms) => (ms != null ? `Rendered in ${formatMs(ms)}` : 'Rendered'),
  },
  repair: {
    icon: RotateCcw,
    running: 'Retrying…',
    done: () => 'Retrying',
  },
}

function ActivityLog({ steps }: { steps: ActivityStep[] }) {
  return (
    <div className="space-y-0.5 py-0.5">
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

  const label = step.status === 'running'
    ? meta.running
    : step.kind === 'repair'
      ? `Retry — ${step.detail ?? 'repairing'}`
      : meta.done(step.ms)

  const row = (
    <div className="flex items-center gap-2 text-[11px] text-dim min-w-0">
      {step.status === 'running' ? (
        <Loader2 size={11} className="animate-spin text-accent flex-shrink-0" />
      ) : (
        <Icon size={11} className="flex-shrink-0 text-muted/60" />
      )}
      <span className="truncate flex-1 text-muted">{label}</span>
      {step.status === 'running' && (
        <span className="text-accent/80 text-[10px] font-mono flex-shrink-0">
          <LiveElapsed startedAt={step.startedAt} />
        </span>
      )}
      {hasThoughts && (
        <ChevronRight
          size={11}
          className={`ml-auto flex-shrink-0 text-dim transition-transform ${open ? 'rotate-90' : ''}`}
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
        <pre className="mt-1 mb-1 ml-5 whitespace-pre-wrap text-[10px] leading-relaxed text-muted/80
                        font-mono border-l border-border pl-2.5 max-h-40 overflow-y-auto">
          {step.detail}
        </pre>
      )}
    </div>
  )
}
