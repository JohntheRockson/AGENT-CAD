import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { SlidersHorizontal, Loader2 } from 'lucide-react'
import { useCadStore } from '../store/useStore'
import {
  formatParameterName,
  parameterAllowsZero,
  parameterEntries,
  parseParameterDraft,
  parseSceneJson,
  sliderBounds,
  unitSuffix,
} from '../lib/document'

/** Treat as unchanged so blur / Enter on the current value does not rebuild. */
function sameParameterValue(a: number, b: number): boolean {
  return Math.abs(a - b) < 1e-9
}

export function ParametersPanel() {
  const irCode         = useCadStore((s) => s.irCode)
  const isRunning      = useCadStore((s) => s.isRunning)
  const timeline       = useCadStore((s) => s.timeline)
  const timelineIndex  = useCadStore((s) => s.timelineIndex)
  const setParameter   = useCadStore((s) => s.setParameter)

  const [open, setOpen] = useState(true)

  const doc = useMemo(() => {
    try {
      return irCode.trim() ? parseSceneJson(irCode) : null
    } catch {
      return null
    }
  }, [irCode])

  const entries = doc ? parameterEntries(doc) : []
  const atTip = timeline.length === 0 || timelineIndex >= timeline.length - 1

  const commit = useCallback(
    (name: string, raw: string, current: number) => {
      const value = parseParameterDraft(raw, name)
      if (value == null) return false
      if (sameParameterValue(value, current)) return false
      void setParameter(name, value)
      return true
    },
    [setParameter],
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
    <div className="absolute top-10 right-2 z-20 w-64 max-h-[min(420px,calc(100%-5rem))]
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
        {entries.map(([name, value]) => (
          <ParameterRow
            key={name}
            name={name}
            value={value}
            disabled={isRunning}
            onCommit={commit}
          />
        ))}
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

function ParameterRow({
  name,
  value,
  disabled,
  onCommit,
}: {
  name: string
  value: number
  disabled: boolean
  onCommit: (name: string, raw: string, current: number) => boolean
}) {
  const [draft, setDraft] = useState(String(value))
  const [editing, setEditing] = useState(false)
  const draftRef = useRef(draft)
  const pendingRef = useRef<number | null>(null)

  const writeDraft = (raw: string) => {
    draftRef.current = raw
    setDraft(raw)
  }

  useEffect(() => {
    if (!editing) {
      writeDraft(String(value))
      if (pendingRef.current != null && sameParameterValue(pendingRef.current, value)) {
        pendingRef.current = null
      }
    }
  }, [value, editing])

  const parsed = Number.parseFloat(draft)
  const allowZero = parameterAllowsZero(name)
  const { min, max } = sliderBounds(value, allowZero)
  const sliderValue = Number.isFinite(parsed) ? parsed : value

  const finishEdit = () => {
    const raw = draftRef.current
    const next = parseParameterDraft(raw, name)
    if (next != null && pendingRef.current != null && sameParameterValue(next, pendingRef.current)) {
      setEditing(false)
      return
    }
    const committed = onCommit(name, raw, value)
    if (committed && next != null) {
      pendingRef.current = next
    } else {
      writeDraft(String(value))
    }
    setEditing(false)
  }

  return (
    <div className="space-y-1">
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
          type="text"
          inputMode="decimal"
          autoComplete="off"
          spellCheck={false}
          value={draft}
          disabled={disabled}
          aria-label={`${formatParameterName(name)} value`}
          onFocus={() => setEditing(true)}
          onChange={(e) => {
            setEditing(true)
            writeDraft(e.target.value)
          }}
          onBlur={finishEdit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              e.currentTarget.blur()
            }
            if (e.key === 'Escape') {
              e.preventDefault()
              writeDraft(String(value))
              setEditing(false)
              e.currentTarget.blur()
            }
          }}
          className="w-[5.25rem] bg-surface border border-border rounded px-1.5 py-0.5
                     text-[11px] text-gray-200 font-mono text-right tabular-nums
                     focus:border-accent/50 focus:outline-none"
        />
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={(max - min) / 200}
        value={sliderValue}
        disabled={disabled}
        aria-label={`${formatParameterName(name)} slider`}
        onPointerDown={(e) => {
          setEditing(true)
          e.currentTarget.setPointerCapture(e.pointerId)
        }}
        onChange={(e) => {
          setEditing(true)
          writeDraft(e.target.value)
        }}
        onPointerUp={finishEdit}
        onBlur={finishEdit}
        onKeyUp={(e) => {
          if (
            e.key === 'ArrowLeft' ||
            e.key === 'ArrowRight' ||
            e.key === 'ArrowUp' ||
            e.key === 'ArrowDown' ||
            e.key === 'Home' ||
            e.key === 'End' ||
            e.key === 'PageUp' ||
            e.key === 'PageDown'
          ) {
            finishEdit()
          }
        }}
        className="w-full h-1 accent-accent cursor-pointer disabled:opacity-40"
      />
    </div>
  )
}
