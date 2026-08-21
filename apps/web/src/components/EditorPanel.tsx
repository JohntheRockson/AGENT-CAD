import { useEffect } from 'react'
import MonacoEditor, { useMonaco } from '@monaco-editor/react'
import { Play, AlertCircle, Loader2, Code2, Braces, AlignLeft, RotateCcw } from 'lucide-react'
import { useCadStore } from '../store/useStore'
import type { MetricsData } from '../types/cad'

// ── JSON Schema for the CAD IR ─────────────────────────────────────────

const FEATURE_ITEM = {
  type: 'object',
  required: ['op'],
  properties: {
    op: {
      type: 'string',
      enum: [
        'sketch','extrude','revolve','cut','fuse','hole','fillet','chamfer','transform',
        'box','cylinder','sphere','cone','torus','loft','mirror','pattern','shell','draft_extrude',
        'thread','sweep','helix','offset','thicken','common','ellipsoid','draft','coil','spring','intersect',
      ],
    },
    id:       { type: 'string' },
    depth:    { type: 'number', minimum: 0.0001 },
    radius:   { type: 'number', minimum: 0.0001 },
    distance: { type: 'number', minimum: 0.0001 },
    diameter: { type: 'number', minimum: 0.0001 },
    plane:    { type: 'string', enum: ['XY', 'XZ', 'YZ'] },
  },
}

const CAD_IR_SCHEMA = {
  $schema: 'http://json-schema.org/draft-07/schema',
  title: 'CadScene',
  oneOf: [
    {
      title: 'CadDocument',
      type: 'object',
      required: ['bodies'],
      properties: {
        documentId: { type: 'string' },
        units: { type: 'string', enum: ['mm', 'in'] },
        bodies: {
          type: 'array',
          items: {
            type: 'object',
            required: ['bodyId', 'features'],
            properties: {
              bodyId:     { type: 'string' },
              name:       { type: 'string' },
              visible:    { type: 'boolean' },
              suppressed: { type: 'boolean' },
              transform: {
                type: 'object',
                properties: {
                  position: { type: 'array', items: { type: 'number' }, minItems: 3, maxItems: 3 },
                  rotation: { type: 'array', items: { type: 'number' }, minItems: 3, maxItems: 3 },
                },
              },
              features: { type: 'array', items: FEATURE_ITEM },
              references: {
                type: 'array',
                items: {
                  type: 'object',
                  required: ['op', 'target'],
                  properties: {
                    op:      { type: 'string', enum: ['cut', 'fuse'] },
                    target:  { type: 'string' },
                    consume: { type: 'boolean' },
                  },
                },
              },
            },
          },
        },
      },
    },
    {
      title: 'CadProgram',
      type: 'object',
      required: ['units', 'features'],
      properties: {
        units:    { type: 'string', enum: ['mm', 'in'] },
        features: { type: 'array', items: FEATURE_ITEM },
      },
    },
  ],
}

// ── Component ──────────────────────────────────────────────────────────

export function EditorPanel() {
  const irCode      = useCadStore((s) => s.irCode)
  const setIrCode   = useCadStore((s) => s.setIrCode)
  const isRunning   = useCadStore((s) => s.isRunning)
  const runError    = useCadStore((s) => s.runError)
  const metrics     = useCadStore((s) => s.metrics)
  const runGeometry = useCadStore((s) => s.runGeometry)
  const clearError  = useCadStore((s) => s.clearError)

  const monaco = useMonaco()

  useEffect(() => {
    if (!monaco) return
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    ;(monaco.languages.json as any).jsonDefaults.setDiagnosticsOptions({
      validate: true,
      schemas: [
        {
          uri: 'https://agentcad.local/cad-ir-schema.json',
          fileMatch: ['*'],
          schema: CAD_IR_SCHEMA,
        },
      ],
    })
  }, [monaco])

  const formatJson = () => {
    try {
      const parsed = JSON.parse(irCode)
      setIrCode(JSON.stringify(parsed, null, 2))
    } catch { /* ignore parse errors */ }
  }

  const charCount = irCode.length
  const lineCount = irCode.split('\n').length

  return (
    <div className="flex flex-col h-full bg-panel">

      {/* ── Toolbar ────────────────────────────────────────────────── */}
      <div className="flex items-center gap-1.5 px-2.5 h-9 border-b border-border flex-shrink-0">
        <Braces size={13} className="text-accent" />
        <span className="text-[11px] font-semibold text-gray-300 tracking-wide uppercase flex-1">
          CAD&nbsp;JSON
        </span>

        {/* Format */}
        <button
          onClick={formatJson}
          disabled={!irCode.trim()}
          title="Format JSON"
          className="p-1.5 rounded text-muted hover:text-gray-200 hover:bg-raised
                     disabled:opacity-30 transition-colors"
        >
          <AlignLeft size={12} />
        </button>

        {/* Clear */}
        <button
          onClick={() => { clearError(); setIrCode('') }}
          disabled={!irCode.trim()}
          title="Clear editor"
          className="p-1.5 rounded text-muted hover:text-red hover:bg-red/10
                     disabled:opacity-30 transition-colors"
        >
          <RotateCcw size={12} />
        </button>

        <div className="h-4 w-px bg-border mx-0.5" />

        {/* Run */}
        <button
          onClick={() => { clearError(); runGeometry() }}
          disabled={isRunning || !irCode.trim()}
          className="flex items-center gap-1.5 px-3 py-1 rounded-md bg-accent text-white
                     text-[11px] font-semibold hover:bg-accent-lite disabled:opacity-30
                     disabled:cursor-not-allowed transition-colors"
        >
          {isRunning
            ? <Loader2 size={11} className="animate-spin" />
            : <Play    size={11} className="fill-current" />
          }
          Run
        </button>
      </div>

      {/* ── Monaco ─────────────────────────────────────────────────── */}
      <div className="flex-1 min-h-0">
        <MonacoEditor
          language="json"
          theme="vs-dark"
          value={irCode}
          onChange={(v) => setIrCode(v ?? '')}
          options={{
            fontSize: 12,
            fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace",
            minimap: { enabled: false },
            lineNumbers: 'on',
            scrollBeyondLastLine: false,
            automaticLayout: true,
            tabSize: 2,
            wordWrap: 'off',
            folding: true,
            renderLineHighlight: 'gutter',
            cursorBlinking: 'smooth',
            lineHeight: 18,
            padding: { top: 8, bottom: 8 },
          }}
        />
      </div>

      {/* ── Error banner ───────────────────────────────────────────── */}
      {runError && (
        <div className="flex items-start gap-2 px-3 py-2 bg-red/8 border-t border-red/20 flex-shrink-0">
          <AlertCircle size={12} className="text-red mt-px flex-shrink-0" />
          <p className="text-[11px] text-red/90 leading-relaxed font-mono break-words">{runError}</p>
        </div>
      )}

      {/* ── Metrics bar ────────────────────────────────────────────── */}
      {metrics && !runError && <MetricsBar metrics={metrics} />}

      {/* ── Footer: line/char count ─────────────────────────────────── */}
      <div className="flex items-center px-3 py-1 bg-surface border-t border-divide flex-shrink-0
                      text-[10px] text-dim font-mono gap-3">
        <span>{lineCount} lines</span>
        <span>{charCount} chars</span>
        <span className="ml-auto text-accent/60">JSON · UTF-8</span>
      </div>
    </div>
  )
}

// ── Metrics bar ────────────────────────────────────────────────────────

function MetricsBar({ metrics }: { metrics: MetricsData }) {
  const [xmin, ymin, zmin, xmax, ymax, zmax] = metrics.bbox
  const size = [xmax - xmin, ymax - ymin, zmax - zmin].map((v) => v.toFixed(1))

  return (
    <div className="flex items-center flex-wrap gap-x-4 gap-y-0.5 px-3 py-1.5 bg-raised
                    border-t border-border flex-shrink-0">
      <MetricItem label="Vol"  value={`${metrics.volume.toFixed(1)} mm³`} />
      <MetricItem label="Area" value={`${metrics.surface_area.toFixed(1)} mm²`} />
      <MetricItem label="Size" value={`${size[0]} × ${size[1]} × ${size[2]} mm`} />
      <span
        className={`ml-auto text-[10px] font-semibold flex items-center gap-1
                    ${metrics.is_solid ? 'text-green' : 'text-amber'}`}
      >
        <span className="w-1.5 h-1.5 rounded-full bg-current" />
        {metrics.is_solid ? 'Solid' : 'Open Shell'}
      </span>
    </div>
  )
}

function MetricItem({ label, value }: { label: string; value: string }) {
  return (
    <span className="text-[10px] font-mono">
      <span className="text-dim">{label}: </span>
      <span className="text-muted">{value}</span>
    </span>
  )
}
