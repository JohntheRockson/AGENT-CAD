import { useEffect, useRef, useState } from 'react'
import { Download, Loader2, ChevronDown, Check, FileBox, FileText, Package, AlertTriangle } from 'lucide-react'
import { useCadStore } from '../store/useStore'
import { canDownloadExport, EXPORT_KINDS } from '../lib/saveFile'
import type { ExportFormat } from '../types/cad'

// ── Format icon mapping ───────────────────────────────────────────────

const FORMAT_ICON: Record<string, typeof FileBox> = {
  step: FileBox,
  stl:  Package,
  gltf: Package,
  obj:  FileText,
  brep: FileBox,
}

const FORMAT_COLOR: Record<string, string> = {
  step: 'text-cyan',
  stl:  'text-green',
  gltf: 'text-amber',
  obj:  'text-muted',
  brep: 'text-accent',
}

// ── Component ──────────────────────────────────────────────────────────

export function ExportMenu() {
  const irCode         = useCadStore((s) => s.irCode)
  const lastGoodIrCode = useCadStore((s) => s.lastGoodIrCode)
  const runError       = useCadStore((s) => s.runError)
  const isRunning      = useCadStore((s) => s.isRunning)
  const isExporting    = useCadStore((s) => s.isExporting)
  const exportStatus   = useCadStore((s) => s.exportStatus)
  const downloadExport = useCadStore((s) => s.downloadExport)

  const [menuOpen, setMenuOpen] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!menuOpen) return
    const onPointer = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false)
      }
    }
    document.addEventListener('mousedown', onPointer)
    return () => document.removeEventListener('mousedown', onPointer)
  }, [menuOpen])

  const gate = canDownloadExport({ runError, irCode, lastGoodIrCode })
  const disabled = isRunning || isExporting || !gate.ok

  return (
    <div className="flex items-center gap-2">
      {/* Export status pill */}
      {exportStatus && (
        <span className="flex items-center gap-1.5 text-[10px] px-2 py-1 rounded-full
                         bg-accent/10 border border-accent/20 text-accent">
          {isExporting
            ? <Loader2 size={9} className="animate-spin" />
            : <Check   size={9} />
          }
          {exportStatus}
        </span>
      )}

      {/* Dropdown */}
      <div className="relative" ref={menuRef}>
        <button
          onClick={() => setMenuOpen((o) => !o)}
          disabled={disabled}
          title={gate.ok ? 'Export' : gate.reason}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-md border border-border
                     text-[11px] font-medium text-gray-200
                     hover:border-accent/50 hover:bg-raised
                     disabled:opacity-30 disabled:cursor-not-allowed transition-all"
        >
          {isExporting
            ? <Loader2 size={12} className="animate-spin text-accent" />
            : <Download size={12} />
          }
          Export
          <ChevronDown
            size={11}
            className={`text-muted transition-transform ${menuOpen ? 'rotate-180' : ''}`}
          />
        </button>

        {menuOpen && (
          <div className="absolute right-0 top-full mt-1.5 w-72 z-50 rounded-xl border border-border
                          bg-panel shadow-cad-lg overflow-hidden">
            {/* Menu header */}
            <div className="px-3 py-2 border-b border-divide">
              <p className="text-[11px] font-semibold text-gray-300">Export Model</p>
              <p className="text-[10px] text-dim mt-0.5">
                {gate.ok ? 'Choose output format' : gate.reason}
              </p>
            </div>

            {/* Format list */}
            <div className="py-1">
              {EXPORT_KINDS.map((kind) => {
                const Icon  = FORMAT_ICON[kind.id] ?? FileBox
                const color = FORMAT_COLOR[kind.id] ?? 'text-muted'
                return (
                  <button
                    key={kind.id}
                    onClick={() => { setMenuOpen(false); downloadExport(kind.id as ExportFormat) }}
                    className="w-full text-left flex items-center gap-3 px-3 py-2.5
                               hover:bg-raised transition-colors group"
                  >
                    <div className={`w-8 h-8 rounded-lg bg-surface border border-divide
                                     flex items-center justify-center flex-shrink-0 ${color}`}>
                      <Icon size={16} strokeWidth={1.5} />
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-[12px] font-semibold text-gray-200">{kind.label}</span>
                        <span className="text-[9px] font-mono text-dim border border-divide rounded px-1">
                          .{kind.ext}
                        </span>
                        {kind.caution && (
                          <span className="inline-flex items-center gap-0.5 text-[9px] font-semibold uppercase tracking-wide text-amber-400/90">
                            <AlertTriangle size={9} />
                            Experimental
                          </span>
                        )}
                      </div>
                      <div className="text-[10px] text-muted mt-0.5 truncate">
                        {kind.caution ?? kind.hint}
                      </div>
                    </div>
                    <Download
                      size={13}
                      className="text-dim group-hover:text-accent transition-colors flex-shrink-0"
                    />
                  </button>
                )
              })}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
