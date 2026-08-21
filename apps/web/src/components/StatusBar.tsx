import { Loader2 } from 'lucide-react'
import { useCadStore } from '../store/useStore'

// ── Helpers ────────────────────────────────────────────────────────────

function fmtVolume(v: number) {
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(2)} cm³`
  if (v >= 1_000)     return `${(v / 1_000).toFixed(2)} cc`
  return `${v.toFixed(1)} mm³`
}

function fmtArea(a: number) {
  if (a >= 1_000_000) return `${(a / 1_000_000).toFixed(2)} cm²`
  return `${a.toFixed(1)} mm²`
}

// ── Status bar ─────────────────────────────────────────────────────────

export function StatusBar() {
  const metrics       = useCadStore((s) => s.metrics)
  const bodies        = useCadStore((s) => s.bodies)
  const isRunning     = useCadStore((s) => s.isRunning)
  const isChatLoading = useCadStore((s) => s.isChatLoading)
  const runError      = useCadStore((s) => s.runError)

  const computing = isRunning || isChatLoading

  const [xmin, ymin, zmin, xmax, ymax, zmax] = metrics?.bbox ?? [0, 0, 0, 0, 0, 0]
  const dims = metrics
    ? [xmax - xmin, ymax - ymin, zmax - zmin].map((d) => d.toFixed(1))
    : null

  return (
    <div
      className="flex-shrink-0 h-6 flex items-center px-3 gap-0 bg-panel border-t border-border
                 text-[10px] font-mono select-none overflow-hidden"
    >
      {/* Status indicator */}
      <div
        className={`flex items-center gap-1.5 min-w-0 pr-3 ${
          computing  ? 'text-accent'
          : runError ? 'text-red-400'
          : 'text-green/80'
        }`}
      >
        {computing ? (
          <Loader2 size={9} className="animate-spin flex-shrink-0" />
        ) : (
          <span
            className={`w-1.5 h-1.5 rounded-full flex-shrink-0 ${
              runError ? 'bg-red' : 'bg-green'
            }`}
          />
        )}
        <span className="truncate">
          {computing  ? (isRunning ? 'Computing geometry…' : 'AI generating…')
          : runError  ? runError.slice(0, 80)
          : 'Ready'}
        </span>
      </div>

      <Sep />

      {/* Body count */}
      <Stat
        label="Bodies"
        value={bodies.length > 0 ? String(bodies.length) : '—'}
      />

      {metrics ? (
        <>
          <Sep />
          <Stat label="Vol"  value={fmtVolume(metrics.volume)} />
          <Sep />
          <Stat label="Area" value={fmtArea(metrics.surface_area)} />
          {dims && (
            <>
              <Sep />
              <Stat label="Dim" value={`${dims[0]} × ${dims[1]} × ${dims[2]} mm`} />
            </>
          )}
        </>
      ) : (
        <>
          <Sep />
          <span className="text-dim">No geometry</span>
        </>
      )}

      <div className="flex-1" />

      {/* Right side */}
      {metrics && (
        <>
          <span
            className={`flex items-center gap-1 ${
              metrics.is_solid ? 'text-green/80' : 'text-amber/80'
            }`}
          >
            <span className="w-1.5 h-1.5 rounded-full bg-current" />
            {metrics.is_solid ? 'Solid' : 'Open Shell'}
          </span>
          <Sep />
        </>
      )}

      <span className="text-muted border border-divide rounded px-1.5 py-px leading-none">
        mm
      </span>
      <Sep />
      <span className="text-dim">AgentCAD</span>
    </div>
  )
}

// ── Sub-components ─────────────────────────────────────────────────────

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <span className="flex items-center gap-1 px-3">
      <span className="text-dim">{label}</span>
      <span className="text-muted">{value}</span>
    </span>
  )
}

function Sep() {
  return <span className="text-divide select-none h-3 flex items-center">│</span>
}
