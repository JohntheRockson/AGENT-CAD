import { useEffect } from 'react'
import MonacoEditor, { useMonaco } from '@monaco-editor/react'
import { Play, AlertCircle, Loader2, Code2 } from 'lucide-react'
import { useCadStore } from '../store/useStore'
import type { MetricsData } from '../types/cad'

// ── JSON Schema for the CAD IR ─────────────────────────────────────────────────

const FEATURE_ITEM = {
  type: 'object',
  required: ['op'],
  properties: {
    op: {
      type: 'string',
      enum: [
        'sketch','extrude','revolve','cut','fuse','hole','fillet','chamfer','transform',
        'box','cylinder','sphere','cone','torus','loft','mirror','pattern','shell','draft_extrude',
        'sweep','pipe','thicken','helix','draft','common',
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
              bodyId: { type: 'string' },
              name: { type: 'string' },
              visible: { type: 'boolean' },
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
                    op: { type: 'string', enum: ['cut', 'fuse'] },
                    target: { type: 'string' },
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
        units: { type: 'string', enum: ['mm', 'in'] },
        features: { type: 'array', items: FEATURE_ITEM },
      },
    },
  ],
}

// ── Component ─────────────────────────────────────────────────────────────────

export function EditorPanel() {
  const irCode       = useCadStore((s) => s.irCode)
  const setIrCode    = useCadStore((s) => s.setIrCode)
  const isRunning    = useCadStore((s) => s.isRunning)
  const runError     = useCadStore((s) => s.runError)
  const metrics      = useCadStore((s) => s.metrics)
  const runGeometry  = useCadStore((s) => s.runGeometry)
  const clearError   = useCadStore((s) => s.clearError)

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

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="flex items-center gap-2 px-3 py-2 border-b border-border bg-panel flex-shrink-0">
        <Code2 size={14} className="text-accent" />
        <span className="text-xs font-semibold text-gray-200 tracking-wide uppercase">
          CAD JSON
        </span>

        <div className="flex-1" />

        {/* Run button */}
        <button
          onClick={() => { clearError(); runGeometry() }}
          disabled={isRunning}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded bg-accent/20 text-accent
                     text-xs font-medium hover:bg-accent/30 disabled:opacity-40
                     disabled:cursor-not-allowed transition-colors"
        >
          {isRunning ? (
            <Loader2 size={12} className="animate-spin" />
          ) : (
            <Play size={12} className="fill-current" />
          )}
          Run Geometry
        </button>
      </div>

      {/* Monaco editor */}
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
          }}
        />
      </div>

      {/* Error banner */}
      {runError && (
        <div className="flex items-start gap-2 px-3 py-2 bg-red-950/60 border-t border-red-800/40 flex-shrink-0">
          <AlertCircle size={13} className="text-red-400 mt-0.5 flex-shrink-0" />
          <p className="text-xs text-red-300 leading-relaxed font-mono">{runError}</p>
        </div>
      )}

      {/* Metrics bar */}
      {metrics && !runError && (
        <MetricsBar metrics={metrics} />
      )}
    </div>
  )
}

function metricSuffixes(units?: MetricsData['units']) {
  if (units === 'in') {
    return { volume: 'in³', length: 'in', area: 'in²' }
  }
  return { volume: 'mm³', length: 'mm', area: 'mm²' }
}

function MetricsBar({ metrics }: { metrics: MetricsData }) {
  const [xmin, ymin, zmin, xmax, ymax, zmax] = metrics.bbox
  const size = [xmax - xmin, ymax - ymin, zmax - zmin].map((v) => v.toFixed(1))
  const suffix = metricSuffixes(metrics.units)

  return (
    <div className="flex items-center gap-4 px-3 py-1.5 bg-surface border-t border-border flex-shrink-0 text-[10px] text-muted font-mono">
      <MetricItem label="Vol" value={`${metrics.volume.toFixed(1)} ${suffix.volume}`} />
      <MetricItem label="Size" value={`${size[0]}×${size[1]}×${size[2]} ${suffix.length}`} />
      <MetricItem label="Area" value={`${metrics.surface_area.toFixed(1)} ${suffix.area}`} />
      <span className={`ml-auto ${metrics.is_solid ? 'text-green-400' : 'text-yellow-400'}`}>
        {metrics.is_solid ? '● solid' : '○ open'}
      </span>
    </div>
  )
}

function MetricItem({ label, value }: { label: string; value: string }) {
  return (
    <span>
      <span className="text-muted/60">{label}: </span>
      <span className="text-gray-300">{value}</span>
    </span>
  )
}
