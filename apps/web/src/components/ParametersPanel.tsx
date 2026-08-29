import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { SlidersHorizontal, Loader2 } from 'lucide-react'
import { useCadStore } from '../store/useStore'
import {
  formatParameterName,
  parameterEntries,
  parseSceneJson,
  unitSuffix,
} from '../lib/document'

export function ParametersPanel() {
  const irCode         = useCadStore((s) => s.irCode)
  const isRunning      = useCadStore((s) => s.isRunning)
  const timeline       = useCadStore((s) => s.timeline)
  const timelineIndex  = useCadStore((s) => s.timelineIndex)
  const setParameter   = useCadStore((s) => s.setParameter)
  const [open, setOpen] = useState(true)
  const [draft, setDraft] = useState<Record<string, string>>({})
  const debounceRef = useRef<Record<string, ReturnType<typeof setTimeout>>>({})

  const doc = useMemo(() => {
    try {
      return irCode.trim() ? parseSceneJson(irCode) : null
    } catch {
      return null
    }
  }, [irCode])

  const entries = doc ? parameterEntries(doc) : []
  const atTip = timeline.length === 0 || timelineIndex >= timeline.length - 1

  useEffect(() => {
    if (!doc) return
    const next: Record<string, string> = {}
    for (const [name, value] of parameterEntries(doc)) {
      next[name] = String(value)
    }
    setDraft(next)
  }, [doc])

  const commit = useCallback(
    (name: string, raw: string) => {
      const value = Number.parseFloat(raw)
      if (!Number.isFinite(value) || value <= 0) return
      void setParameter(name, value)
    },
    [setParameter],
  )

  const queueCommit = useCallback(
    (name: string, raw: string) => {
      setDraft((d) => ({ ...d, [name]: raw }))
      clearTimeout(debounceRef.current[name])
      debounceRef.current[name] = setTimeout(() => commit(name, raw), 350)
    },
    [commit],
  )

  if (!doc || entries.length === 0) {
    return null
  }

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="absolute top-10 right-2 z-20 flex items-center gap-1.5 px-2 py-1 rounded-md
                   bg-panel/90 border border-border text-[11px] text-muted hover:text-gray-200"
        title="Show parameters"
      >
        <SlidersHorizontal size={12} />
        Parameters
      </button>
    )
  }

  return (
    <div className="absolute top-10 right-2 z-20 w-60 max-h-[min(420px,calc(100%-5rem))]
                    flex flex-col rounded-lg border border-border bg-panel/95 shadow-lg backdrop-blur-sm">
      <div className="flex items-center gap-1.5 px-2.5 py-1.5 border-b border-border">
        <SlidersHorizontal size={12} className="text-accent" />
        <span className="text-[10px] font-semibold uppercase tracking-wide text-gray-300">
          Parameters
        </span>
        <span className="ml-auto text-[10px] text-muted">{unitSuffix(doc.units)}</span>
        <button
          type="button"
          onClick={() => setOpen(false)}
          className="text-muted hover:text-gray-200 text-xs px-1"
          title="Collapse"
        >
          ×
        </button>
      </div>

      {!atTip && (
        <p className="px-2.5 py-1.5 text-[10px] text-yellow-400/90 border-b border-border leading-snug">
          Historical step — edits branch from here and replace later timeline.
        </p>
      )}

      <div className="overflow-y-auto py-2 px-2.5 space-y-3 min-h-0">
        {entries.map(([name, value]) => {
          const min = Math.max(0.1, value * 0.1)
          const max = Math.max(min * 2, value * 3)
          const draftVal = draft[name] ?? String(value)
          const parsed = Number.parseFloat(draftVal)
          return (
            <div key={name} className="space-y-1">
              <div className="flex items-center justify-between gap-2">
                <label
                  htmlFor={`param-${name}`}
                  className="text-[10px] text-gray-300 capitalize truncate"
                  title={name}
                >
                  {formatParameterName(name)}
                </label>
                <input
                  id={`param-${name}`}
                  type="number"
                  min={0.01}
                  step="any"
                  value={draftVal}
                  disabled={isRunning}
                  onChange={(e) => queueCommit(name, e.target.value)}
                  onBlur={() => commit(name, draftVal)}
                  className="w-[4.5rem] bg-surface border border-border rounded px-1.5 py-0.5
                             text-[10px] text-gray-200 font-mono text-right focus:border-accent/50 focus:outline-none"
                />
              </div>
              <input
                type="range"
                min={min}
                max={max}
                step={(max - min) / 200}
                value={Number.isFinite(parsed) ? parsed : value}
                disabled={isRunning}
                onChange={(e) => queueCommit(name, e.target.value)}
                className="w-full h-1 accent-accent cursor-pointer disabled:opacity-40"
              />
            </div>
          )
        })}
      </div>

      {isRunning && (
        <div className="flex items-center gap-1.5 px-2.5 py-1.5 border-t border-border text-[10px] text-accent">
          <Loader2 size={10} className="animate-spin" />
          Rebuilding…
        </div>
      )}
    </div>
  )
}
