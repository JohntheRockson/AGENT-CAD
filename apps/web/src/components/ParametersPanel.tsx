import { useCallback, useMemo, useState } from 'react'
import { SlidersHorizontal, Loader2, Trash2, RotateCcw } from 'lucide-react'
import { useCadStore } from '../store/useStore'
import {
  collectParameterBatch,
  explicitParameterNames,
  formatParameterName,
  isExplicitParameter,
  parameterAllowsZero,
  parameterEntries,
  parseParameterDraft,
  parseSceneJson,
  sameParameterValue,
  sliderBounds,
  unitSuffix,
} from '../lib/document'

export function ParametersPanel() {
  const irCode              = useCadStore((s) => s.irCode)
  const isRunning           = useCadStore((s) => s.isRunning)
  const timeline            = useCadStore((s) => s.timeline)
  const timelineIndex       = useCadStore((s) => s.timelineIndex)
  const calculateParameters = useCadStore((s) => s.calculateParameters)

  const [open, setOpen] = useState(true)
  const [drafts, setDrafts] = useState<Record<string, string>>({})
  const [pendingDeletes, setPendingDeletes] = useState<string[]>([])
  const [draftIr, setDraftIr] = useState(irCode)

  // New committed IR (Calculate success, chat, timeline) clears local drafts.
  if (draftIr !== irCode) {
    setDraftIr(irCode)
    setDrafts({})
    setPendingDeletes([])
  }

  const doc = useMemo(() => {
    try {
      return irCode.trim() ? parseSceneJson(irCode) : null
    } catch {
      return null
    }
  }, [irCode])

  const entries = doc ? parameterEntries(doc) : []
  const explicitNames = doc ? explicitParameterNames(doc) : []
  const committed = useMemo(
    () => Object.fromEntries(entries),
    [entries],
  )
  const atTip = timeline.length === 0 || timelineIndex >= timeline.length - 1

  const batch = useMemo(
    () =>
      collectParameterBatch({
        committed,
        explicitNames,
        drafts,
        pendingDeletes,
      }),
    [committed, explicitNames, drafts, pendingDeletes],
  )

  const dirtyCount = Object.keys(batch.values).length + batch.deletes.length
  const canCalculate = dirtyCount > 0 && batch.invalid.length === 0 && !isRunning

  const setDraft = useCallback((name: string, raw: string) => {
    setDrafts((prev) => ({ ...prev, [name]: raw }))
  }, [])

  const revertDraft = useCallback((name: string) => {
    setDrafts((prev) => {
      const next = { ...prev }
      delete next[name]
      return next
    })
  }, [])

  const markDelete = useCallback((name: string) => {
    setPendingDeletes((prev) => (prev.includes(name) ? prev : [...prev, name]))
  }, [])

  const restoreDelete = useCallback((name: string) => {
    setPendingDeletes((prev) => prev.filter((n) => n !== name))
  }, [])

  const onCalculate = useCallback(() => {
    if (!canCalculate) return
    void calculateParameters({
      values: batch.values,
      deletes: batch.deletes,
    })
  }, [batch.deletes, batch.values, calculateParameters, canCalculate])

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
    <div className="absolute top-10 right-2 z-20 w-72 max-h-[min(460px,calc(100%-5rem))]
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
          const pendingDelete = pendingDeletes.includes(name)
          const draft = drafts[name] ?? String(value)
          const parsed = parseParameterDraft(draft, name)
          const dirtyValue =
            !pendingDelete && parsed != null && !sameParameterValue(parsed, value)
          const invalid = !pendingDelete && drafts[name] != null && parsed == null
          const explicit = isExplicitParameter(doc, name)
          return (
            <ParameterRow
              key={name}
              name={name}
              committed={value}
              draft={draft}
              disabled={isRunning}
              dirty={dirtyValue}
              invalid={invalid}
              pendingDelete={pendingDelete}
              canDelete={explicit}
              onDraftChange={(raw) => setDraft(name, raw)}
              onRevert={() => revertDraft(name)}
              onDelete={() => markDelete(name)}
              onRestore={() => restoreDelete(name)}
            />
          )
        })}
      </div>

      <div className="flex items-center gap-2 px-2.5 py-1.5 border-t border-border">
        <button
          type="button"
          onClick={onCalculate}
          disabled={!canCalculate}
          className="flex items-center gap-1.5 px-2.5 py-1 rounded-md text-[11px] font-semibold
                     bg-accent text-white hover:bg-accent-lite disabled:opacity-25
                     disabled:cursor-not-allowed transition-all"
          title={
            batch.invalid.length
              ? 'Fix invalid drafts before calculating'
              : dirtyCount
                ? 'Commit all dirty parameters and rebuild once'
                : 'No uncommitted parameter changes'
          }
        >
          {isRunning ? <Loader2 size={10} className="animate-spin" /> : null}
          Calculate
        </button>
        {isRunning ? (
          <span className="text-[10px] text-accent">Rebuilding…</span>
        ) : dirtyCount > 0 ? (
          <span className="text-[10px] text-muted">
            {dirtyCount} uncommitted
          </span>
        ) : null}
      </div>
    </div>
  )
}

function ParameterRow({
  name,
  committed,
  draft,
  disabled,
  dirty,
  invalid,
  pendingDelete,
  canDelete,
  onDraftChange,
  onRevert,
  onDelete,
  onRestore,
}: {
  name: string
  committed: number
  draft: string
  disabled: boolean
  dirty: boolean
  invalid: boolean
  pendingDelete: boolean
  canDelete: boolean
  onDraftChange: (raw: string) => void
  onRevert: () => void
  onDelete: () => void
  onRestore: () => void
}) {
  const allowZero = parameterAllowsZero(name)
  const parsed = Number.parseFloat(draft)
  const { min, max } = sliderBounds(committed, allowZero)
  const sliderValue = Number.isFinite(parsed) ? parsed : committed
  const rowDisabled = disabled || pendingDelete

  return (
    <div className={`space-y-1 ${pendingDelete ? 'opacity-50' : ''}`}>
      <div className="flex items-center justify-between gap-1.5">
        <label
          htmlFor={`param-${name}`}
          className={`text-[10px] text-gray-300 capitalize truncate min-w-0
                      ${pendingDelete ? 'line-through' : ''}`}
          title={name}
        >
          {formatParameterName(name)}
          {dirty && !pendingDelete ? (
            <span className="ml-0.5 text-accent" aria-hidden>*</span>
          ) : null}
        </label>
        <div className="flex items-center gap-1 shrink-0">
          <input
            id={`param-${name}`}
            type="text"
            inputMode="decimal"
            autoComplete="off"
            spellCheck={false}
            value={draft}
            disabled={rowDisabled}
            aria-label={`${formatParameterName(name)} value`}
            aria-invalid={invalid || undefined}
            onChange={(e) => onDraftChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                e.currentTarget.blur()
              }
              if (e.key === 'Escape') {
                e.preventDefault()
                onRevert()
                e.currentTarget.blur()
              }
            }}
            className={`w-[4.75rem] bg-surface border rounded px-1.5 py-0.5
                       text-[11px] text-gray-200 font-mono text-right tabular-nums
                       focus:outline-none
                       ${invalid
                         ? 'border-red-500/70 focus:border-red-400'
                         : dirty
                           ? 'border-accent/60 focus:border-accent'
                           : 'border-border focus:border-accent/50'}`}
          />
          {pendingDelete ? (
            <button
              type="button"
              onClick={onRestore}
              disabled={disabled}
              className="p-0.5 text-muted hover:text-accent disabled:opacity-40"
              title={`Restore ${formatParameterName(name)}`}
              aria-label={`Restore ${formatParameterName(name)}`}
            >
              <RotateCcw size={11} />
            </button>
          ) : (
            <button
              type="button"
              onClick={onDelete}
              disabled={disabled || !canDelete}
              className="p-0.5 text-muted hover:text-red-400 disabled:opacity-30
                         disabled:hover:text-muted disabled:cursor-not-allowed"
              title={
                canDelete
                  ? `Delete ${formatParameterName(name)} from the parameters map`
                  : 'Inferred from geometry — not in the parameters map'
              }
              aria-label={
                canDelete
                  ? `Delete ${formatParameterName(name)}`
                  : `${formatParameterName(name)} is inferred and cannot be deleted`
              }
            >
              <Trash2 size={11} />
            </button>
          )}
        </div>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={(max - min) / 200}
        value={sliderValue}
        disabled={rowDisabled}
        aria-label={`${formatParameterName(name)} slider`}
        onChange={(e) => onDraftChange(e.target.value)}
        className="w-full h-1 accent-accent cursor-pointer disabled:opacity-40"
      />
    </div>
  )
}
