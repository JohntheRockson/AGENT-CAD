import { useEffect, useMemo, useState, type ReactNode } from 'react'
import {
  Box, ChevronDown, ChevronRight, Eye, EyeOff, Focus, Pencil, Trash2, ListTree,
} from 'lucide-react'
import { useCadStore } from '../store/useStore'
import { bodyColor, featureKey, featureLabel, parseSceneJson } from '../lib/document'
import type { Feature } from '../types/cad'

export function Outliner() {
  const irCode          = useCadStore((s) => s.irCode)
  const bodies          = useCadStore((s) => s.bodies)
  const selectedBodyId  = useCadStore((s) => s.selectedBodyId)
  const hoveredBodyId   = useCadStore((s) => s.hoveredBodyId)
  const isolatedBodyId  = useCadStore((s) => s.isolatedBodyId)
  const outlinerOpen    = useCadStore((s) => s.outlinerOpen)
  const setOutlinerOpen = useCadStore((s) => s.setOutlinerOpen)
  const selectBody      = useCadStore((s) => s.selectBody)
  const hoverBody       = useCadStore((s) => s.hoverBody)
  const setBodyVisible  = useCadStore((s) => s.setBodyVisible)
  const isolateBody     = useCadStore((s) => s.isolateBody)
  const renameBody      = useCadStore((s) => s.renameBody)
  const deleteBody      = useCadStore((s) => s.deleteBody)

  const doc = useMemo(() => {
    try {
      return irCode.trim() ? parseSceneJson(irCode) : null
    } catch {
      return null
    }
  }, [irCode])

  const nodes = doc?.bodies ?? bodies.map((b) => ({
    bodyId: b.bodyId,
    name: b.name,
    visible: b.visible,
    suppressed: b.suppressed,
    features: [] as Feature[],
  }))

  if (!outlinerOpen) {
    return (
      <button
        type="button"
        onClick={() => setOutlinerOpen(true)}
        className="absolute top-10 left-2 z-20 flex items-center gap-1.5 px-2 py-1 rounded-md
                   bg-panel/90 border border-border text-[11px] text-muted hover:text-gray-200"
        title="Show outliner"
      >
        <ListTree size={12} />
        Outliner
      </button>
    )
  }

  return (
    <div className="absolute top-10 left-2 z-20 w-56 max-h-[min(420px,calc(100%-3.5rem))]
                    flex flex-col rounded-lg border border-border bg-panel/95 shadow-lg backdrop-blur-sm">
      <div className="flex items-center gap-1.5 px-2.5 py-1.5 border-b border-border">
        <ListTree size={12} className="text-accent" />
        <span className="text-[10px] font-semibold uppercase tracking-wide text-gray-300">
          Outliner
        </span>
        <span className="ml-auto text-[10px] text-muted">{nodes.length}</span>
        <button
          type="button"
          onClick={() => setOutlinerOpen(false)}
          className="text-muted hover:text-gray-200 text-xs px-1"
          title="Collapse"
        >
          ×
        </button>
      </div>

      <div className="overflow-y-auto py-1 min-h-0">
        {nodes.length === 0 && (
          <p className="px-3 py-4 text-[11px] text-muted text-center">
            No bodies yet. Describe an assembly in chat.
          </p>
        )}
        {nodes.map((node, index) => {
          const inst = bodies.find((b) => b.bodyId === node.bodyId)
          const visible = inst?.visible ?? node.visible !== false
          const selected = selectedBodyId === node.bodyId
          const hovered = hoveredBodyId === node.bodyId
          const isolated = isolatedBodyId === node.bodyId
          return (
            <BodyNode
              key={node.bodyId}
              bodyId={node.bodyId}
              name={node.name || node.bodyId}
              features={node.features ?? []}
              color={bodyColor(index)}
              visible={visible}
              selected={selected}
              hovered={hovered}
              isolated={isolated}
              suppressed={!!node.suppressed}
              onSelect={() => selectBody(selected ? null : node.bodyId)}
              onHover={(h) => hoverBody(h ? node.bodyId : null)}
              onToggleVisible={() => setBodyVisible(node.bodyId, !visible)}
              onIsolate={() => isolateBody(node.bodyId)}
              onRename={(name) => renameBody(node.bodyId, name)}
              onDelete={() => deleteBody(node.bodyId)}
            />
          )
        })}
      </div>
    </div>
  )
}

function BodyNode({
  bodyId,
  name,
  features,
  color,
  visible,
  selected,
  hovered,
  isolated,
  suppressed,
  onSelect,
  onHover,
  onToggleVisible,
  onIsolate,
  onRename,
  onDelete,
}: {
  bodyId: string
  name: string
  features: Feature[]
  color: string
  visible: boolean
  selected: boolean
  hovered: boolean
  isolated: boolean
  suppressed: boolean
  onSelect: () => void
  onHover: (h: boolean) => void
  onToggleVisible: () => void
  onIsolate: () => void
  onRename: (name: string) => void
  onDelete: () => void
}) {
  const [open, setOpen] = useState(selected)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(name)

  useEffect(() => {
    if (selected) setOpen(true)
  }, [selected])

  return (
    <div
      className={`group px-1 ${selected ? 'bg-accent/15' : hovered ? 'bg-white/5' : ''}`}
      onMouseEnter={() => onHover(true)}
      onMouseLeave={() => onHover(false)}
    >
      <div className="flex items-center gap-0.5 py-0.5">
        <button
          type="button"
          className="p-0.5 text-muted"
          onClick={() => setOpen((v) => !v)}
        >
          {open ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
        </button>
        <Box size={12} style={{ color }} className="flex-shrink-0" />
        {editing ? (
          <input
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => {
              setEditing(false)
              if (draft.trim()) onRename(draft.trim())
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
              if (e.key === 'Escape') setEditing(false)
            }}
            className="flex-1 min-w-0 bg-surface border border-accent/40 rounded px-1 py-0.5 text-[11px] text-gray-200"
          />
        ) : (
          <button
            type="button"
            onClick={onSelect}
            className={`flex-1 min-w-0 text-left text-[11px] truncate ${
              suppressed ? 'text-muted line-through' : 'text-gray-200'
            }`}
            title={bodyId}
          >
            {name}
          </button>
        )}
        <div className="flex items-center opacity-0 group-hover:opacity-100 focus-within:opacity-100">
          <IconBtn title={visible ? 'Hide' : 'Show'} onClick={onToggleVisible}>
            {visible ? <Eye size={11} /> : <EyeOff size={11} />}
          </IconBtn>
          <IconBtn title={isolated ? 'Show all' : 'Isolate'} onClick={onIsolate}>
            <Focus size={11} className={isolated ? 'text-accent' : ''} />
          </IconBtn>
          <IconBtn
            title="Rename"
            onClick={() => {
              setDraft(name)
              setEditing(true)
            }}
          >
            <Pencil size={11} />
          </IconBtn>
          <IconBtn title="Delete" onClick={onDelete}>
            <Trash2 size={11} />
          </IconBtn>
        </div>
      </div>
      {open && features.length > 0 && (
        <ul className="ml-6 mb-1 border-l border-border pl-2 space-y-0.5">
          {features.map((f, i) => (
            <li key={featureKey(f, i)} className="text-[10px] text-muted truncate">
              {featureLabel(f, i)}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

function IconBtn({
  title,
  onClick,
  children,
}: {
  title: string
  onClick: () => void
  children: ReactNode
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={(e) => {
        e.stopPropagation()
        onClick()
      }}
      className="p-0.5 rounded text-muted hover:text-gray-200 hover:bg-white/10"
    >
      {children}
    </button>
  )
}
