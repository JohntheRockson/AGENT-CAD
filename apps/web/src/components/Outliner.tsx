import { useEffect, useMemo, useState, type ReactNode } from 'react'
import {
  Box, ChevronDown, ChevronRight, Eye, EyeOff, Focus, Pencil, Trash2,
  Layers, Search, ListTree, LayoutList,
} from 'lucide-react'
import { useCadStore } from '../store/useStore'
import { bodyColor, featureKey, featureLabel, parseSceneJson } from '../lib/document'
import type { Feature } from '../types/cad'

// ── Tabs ───────────────────────────────────────────────────────────────

type Tab = 'bodies' | 'features'

// ── Component ──────────────────────────────────────────────────────────

export function Outliner() {
  const irCode          = useCadStore((s) => s.irCode)
  const bodies          = useCadStore((s) => s.bodies)
  const selectedBodyId  = useCadStore((s) => s.selectedBodyId)
  const hoveredBodyId   = useCadStore((s) => s.hoveredBodyId)
  const isolatedBodyId  = useCadStore((s) => s.isolatedBodyId)
  const selectBody      = useCadStore((s) => s.selectBody)
  const hoverBody       = useCadStore((s) => s.hoverBody)
  const setBodyVisible  = useCadStore((s) => s.setBodyVisible)
  const isolateBody     = useCadStore((s) => s.isolateBody)
  const renameBody      = useCadStore((s) => s.renameBody)
  const deleteBody      = useCadStore((s) => s.deleteBody)

  const [tab, setTab]     = useState<Tab>('bodies')
  const [filter, setFilter] = useState('')

  const doc = useMemo(() => {
    try { return irCode.trim() ? parseSceneJson(irCode) : null } catch { return null }
  }, [irCode])

  const nodes = doc?.bodies ?? bodies.map((b) => ({
    bodyId: b.bodyId,
    name: b.name,
    visible: b.visible,
    suppressed: b.suppressed,
    features: [] as Feature[],
  }))

  const filtered = filter.trim()
    ? nodes.filter((n) => (n.name || n.bodyId).toLowerCase().includes(filter.toLowerCase()))
    : nodes

  // All features across all bodies (for the Features tab)
  const allFeatures = useMemo(
    () => nodes.flatMap((n) => (n.features ?? []).map((f, i) => ({ f, i, bodyId: n.bodyId, bodyName: n.name }))),
    [nodes],
  )

  return (
    <div className="flex flex-col h-full min-h-0 bg-panel">

      {/* ── Panel header ───────────────────────────────────────────── */}
      <div className="flex items-center gap-1.5 px-3 h-9 border-b border-border flex-shrink-0">
        <Layers size={12} className="text-accent" />
        <span className="text-[11px] font-semibold text-gray-300 tracking-wide uppercase flex-1">
          Model Tree
        </span>
        <span className="text-[9px] text-dim bg-surface rounded px-1.5 py-0.5 font-mono">
          {nodes.length}
        </span>
      </div>

      {/* ── Tabs ───────────────────────────────────────────────────── */}
      <div className="flex border-b border-border flex-shrink-0">
        <TabBtn label="Bodies"   icon={<Box size={11} />}       active={tab === 'bodies'}   onClick={() => setTab('bodies')} />
        <TabBtn label="Features" icon={<ListTree size={11} />}  active={tab === 'features'} onClick={() => setTab('features')} />
      </div>

      {/* ── Search / filter ────────────────────────────────────────── */}
      <div className="px-2 py-1.5 border-b border-divide flex-shrink-0">
        <div className="flex items-center gap-1.5 bg-surface rounded px-2 py-1 border border-divide
                        focus-within:border-accent/40 transition-colors">
          <Search size={10} className="text-dim flex-shrink-0" />
          <input
            type="text"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Filter…"
            className="flex-1 bg-transparent text-[11px] text-gray-300 placeholder-dim
                       outline-none min-w-0"
          />
        </div>
      </div>

      {/* ── Body list ──────────────────────────────────────────────── */}
      {tab === 'bodies' && (
        <div className="flex-1 overflow-y-auto py-1 min-h-0">
          {filtered.length === 0 && (
            <div className="flex flex-col items-center justify-center py-8 gap-2">
              <Box size={28} className="text-border" strokeWidth={1} />
              <p className="text-[11px] text-dim text-center leading-relaxed px-4">
                {filter ? 'No matches' : 'No bodies yet.\nDescribe a part in the AI chat.'}
              </p>
            </div>
          )}
          {filtered.map((node, index) => {
            const inst    = bodies.find((b) => b.bodyId === node.bodyId)
            const visible = inst?.visible ?? node.visible !== false
            const selected = selectedBodyId === node.bodyId
            const hovered  = hoveredBodyId  === node.bodyId
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
      )}

      {/* ── All features flat list ─────────────────────────────────── */}
      {tab === 'features' && (
        <div className="flex-1 overflow-y-auto py-1 min-h-0">
          {allFeatures.length === 0 && (
            <p className="text-[11px] text-dim text-center py-8">No features</p>
          )}
          {allFeatures.map(({ f, i, bodyId, bodyName }) => (
            <div
              key={`${bodyId}-${featureKey(f, i)}`}
              className="flex items-center gap-2 px-3 py-1 hover:bg-raised"
            >
              <span
                className="w-1.5 h-1.5 rounded-full flex-shrink-0"
                style={{ background: bodyColor(bodies.findIndex((b) => b.bodyId === bodyId)) }}
              />
              <span className="text-[11px] text-muted truncate flex-1">{featureLabel(f, i)}</span>
              <span className="text-[9px] text-dim shrink-0">{bodyName || bodyId}</span>
            </div>
          ))}
        </div>
      )}

      {/* ── Footer stats ───────────────────────────────────────────── */}
      <div className="border-t border-border px-3 py-1.5 flex items-center gap-3 flex-shrink-0">
        <span className="text-[10px] text-dim">
          <span className="text-muted">{nodes.length}</span> bod{nodes.length === 1 ? 'y' : 'ies'}
        </span>
        <span className="text-[10px] text-dim">
          <span className="text-muted">{allFeatures.length}</span> feat{allFeatures.length === 1 ? 'ure' : 'ures'}
        </span>
        {isolatedBodyId && (
          <span className="ml-auto text-[9px] px-1.5 py-0.5 rounded bg-accent/15 text-accent border border-accent/20">
            Isolated
          </span>
        )}
      </div>
    </div>
  )
}

// ── Tab button ─────────────────────────────────────────────────────────

function TabBtn({
  label, icon, active, onClick,
}: {
  label: string
  icon: ReactNode
  active: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex-1 flex items-center justify-center gap-1.5 py-1.5 text-[10px] font-medium
                  transition-colors border-b-2 ${
        active
          ? 'border-accent text-accent'
          : 'border-transparent text-muted hover:text-gray-300'
      }`}
    >
      {icon}
      {label}
    </button>
  )
}

// ── Body node ──────────────────────────────────────────────────────────

function BodyNode({
  bodyId, name, features, color, visible, selected, hovered,
  isolated, suppressed,
  onSelect, onHover, onToggleVisible, onIsolate, onRename, onDelete,
}: {
  bodyId: string; name: string; features: Feature[]
  color: string; visible: boolean; selected: boolean; hovered: boolean
  isolated: boolean; suppressed: boolean
  onSelect: () => void; onHover: (h: boolean) => void
  onToggleVisible: () => void; onIsolate: () => void
  onRename: (name: string) => void; onDelete: () => void
}) {
  const [open, setOpen]     = useState(false)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft]   = useState(name)

  useEffect(() => { if (selected) setOpen(true) }, [selected])
  useEffect(() => { setDraft(name) }, [name])

  return (
    <div
      onMouseEnter={() => onHover(true)}
      onMouseLeave={() => onHover(false)}
    >
      {/* Row */}
      <div
        className={`flex items-center gap-0.5 px-1.5 py-0.5 group cursor-pointer
                    ${selected ? 'bg-accent/12' : hovered ? 'bg-raised' : ''}`}
      >
        {/* Expand toggle */}
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          className="p-0.5 text-dim hover:text-muted rounded flex-shrink-0"
        >
          {open
            ? <ChevronDown size={10} />
            : <ChevronRight size={10} />
          }
        </button>

        {/* Color swatch */}
        <span
          className="w-2.5 h-2.5 rounded-sm flex-shrink-0"
          style={{ background: color }}
        />

        {/* Name / edit */}
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
              if (e.key === 'Escape') { setEditing(false); setDraft(name) }
            }}
            className="flex-1 min-w-0 bg-surface border border-accent/40 rounded px-1 py-px
                       text-[11px] text-gray-200 outline-none"
          />
        ) : (
          <button
            type="button"
            onClick={onSelect}
            className={`flex-1 min-w-0 text-left text-[11px] truncate transition-colors
                        ${suppressed ? 'line-through text-dim' : selected ? 'text-gray-100' : 'text-gray-300'}`}
            title={bodyId}
          >
            {name}
          </button>
        )}

        {/* Action icons — visible on hover */}
        <div className="flex items-center gap-px opacity-0 group-hover:opacity-100 focus-within:opacity-100">
          <IconBtn title={visible ? 'Hide' : 'Show'} onClick={onToggleVisible}>
            {visible ? <Eye size={10} /> : <EyeOff size={10} className="text-dim" />}
          </IconBtn>
          <IconBtn title={isolated ? 'Show all' : 'Isolate'} onClick={onIsolate}>
            <Focus size={10} className={isolated ? 'text-accent' : ''} />
          </IconBtn>
          <IconBtn title="Rename" onClick={() => { setDraft(name); setEditing(true) }}>
            <Pencil size={10} />
          </IconBtn>
          <IconBtn title="Delete" onClick={onDelete} danger>
            <Trash2 size={10} />
          </IconBtn>
        </div>
      </div>

      {/* Feature sub-list */}
      {open && features.length > 0 && (
        <ul className="ml-7 mb-0.5 border-l border-divide pl-2 space-y-px">
          {features.map((f, i) => (
            <li key={featureKey(f, i)}
                className="text-[10px] text-dim hover:text-muted truncate py-px pl-1 transition-colors">
              {featureLabel(f, i)}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

// ── Tiny icon button ───────────────────────────────────────────────────

function IconBtn({
  title, onClick, children, danger = false,
}: {
  title: string; onClick: () => void; children: ReactNode; danger?: boolean
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={(e) => { e.stopPropagation(); onClick() }}
      className={`p-0.5 rounded transition-colors
        ${danger
          ? 'text-dim hover:text-red hover:bg-red/10'
          : 'text-dim hover:text-gray-200 hover:bg-white/10'
        }`}
    >
      {children}
    </button>
  )
}
