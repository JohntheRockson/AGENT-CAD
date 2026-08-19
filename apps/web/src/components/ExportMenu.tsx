import { useEffect, useRef, useState } from 'react'
import { Download, Loader2, ChevronDown, Check } from 'lucide-react'
import { useCadStore } from '../store/useStore'
import { EXPORT_KINDS } from '../lib/saveFile'

export function ExportMenu() {
  const irCode         = useCadStore((s) => s.irCode)
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

  return (
    <div className="flex items-center gap-2">
      {exportStatus && (
        <span className="flex items-center gap-1.5 text-[10px] text-accent">
          {isExporting && <Loader2 size={10} className="animate-spin" />}
          {!isExporting && <Check size={10} />}
          {exportStatus}
        </span>
      )}

      <div className="relative" ref={menuRef}>
        <button
          onClick={() => setMenuOpen((o) => !o)}
          disabled={isRunning || isExporting || !irCode.trim()}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded border border-border text-gray-200
                     text-xs font-medium hover:border-accent/50 hover:bg-accent/10
                     disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          {isExporting ? (
            <Loader2 size={12} className="animate-spin text-accent" />
          ) : (
            <Download size={12} />
          )}
          Export
          <ChevronDown size={11} className={`text-muted transition-transform ${menuOpen ? 'rotate-180' : ''}`} />
        </button>

        {menuOpen && (
          <div className="absolute right-0 top-full mt-1 w-64 z-50 rounded-md border border-border
                          bg-panel shadow-xl overflow-hidden">
            {EXPORT_KINDS.map((kind) => (
              <button
                key={kind.id}
                onClick={() => {
                  setMenuOpen(false)
                  downloadExport(kind.id)
                }}
                className="w-full text-left px-3 py-2 hover:bg-accent/10 transition-colors
                           border-b border-border last:border-b-0"
              >
                <div className="text-xs text-gray-200 font-medium">{kind.label}</div>
                <div className="text-[10px] text-muted mt-0.5">{kind.hint}</div>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
