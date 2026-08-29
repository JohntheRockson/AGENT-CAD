import type { LucideIcon } from 'lucide-react'
import {
  Box, Circle, Globe, Triangle, RotateCcw, CircleDot,
  ArrowUp, RefreshCw, Layers, Package, Spline, Orbit,
  CornerDownRight, Minus, FlipHorizontal, LayoutGrid, Move, Expand,
  Scissors, GitMerge, Target, Nut, Blend,
  Code2, PanelLeft, MessageSquare,
  Play, Loader2, Settings,
} from 'lucide-react'
import { ExportMenu } from './ExportMenu'
import { useCadStore } from '../store/useStore'

// ── Tool definitions ───────────────────────────────────────────────────

interface ToolDef {
  icon: LucideIcon
  label: string
  prompt: string
}

interface ToolGroup {
  label: string
  tools: ToolDef[]
}

const TOOL_GROUPS: ToolGroup[] = [
  {
    label: 'Primitives',
    tools: [
      { icon: Box,             label: 'Box',      prompt: 'Create a box primitive, 50mm × 50mm × 50mm' },
      { icon: Circle,          label: 'Cylinder', prompt: 'Create a cylinder, 30mm diameter, 60mm height' },
      { icon: Globe,           label: 'Sphere',   prompt: 'Create a sphere, 50mm diameter' },
      { icon: Triangle,        label: 'Cone',     prompt: 'Create a cone, 40mm base diameter, 60mm height' },
      { icon: RotateCcw,       label: 'Torus',     prompt: 'Create a torus, 35mm major radius, 8mm minor radius' },
      { icon: CircleDot,       label: 'Ellipsoid', prompt: 'Create an ellipsoid with radii 20, 12, and 8 mm' },
    ],
  },
  {
    label: 'Create',
    tools: [
      { icon: ArrowUp,         label: 'Extrude',  prompt: 'Extrude the current sketch profile 20mm upward' },
      { icon: RefreshCw,       label: 'Revolve',  prompt: 'Revolve the profile 360° around the Z axis' },
      { icon: Layers,          label: 'Loft',     prompt: 'Create a loft sweep between two profiles' },
      { icon: Package,         label: 'Shell',    prompt: 'Shell the model with 2mm wall thickness, removing the top face' },
      { icon: Spline,          label: 'Sweep',    prompt: 'Sweep a 4mm circle along a helix, pitch 8mm, radius 12mm, height 40mm' },
      { icon: Orbit,           label: 'Helix',    prompt: 'Create a spring: helix radius 10mm, pitch 5mm, height 40mm, wire diameter 2mm' },
    ],
  },
  {
    label: 'Modify',
    tools: [
      { icon: CornerDownRight, label: 'Fillet',   prompt: 'Add 3mm radius fillets to all edges' },
      { icon: Minus,           label: 'Chamfer',  prompt: 'Add 2mm chamfers to all edges' },
      { icon: FlipHorizontal,  label: 'Mirror',   prompt: 'Mirror the body symmetrically across the XZ plane' },
      { icon: LayoutGrid,      label: 'Pattern',  prompt: 'Create a 3×3 rectangular pattern with 20mm spacing' },
      { icon: Move,            label: 'Transform',prompt: 'Translate the body 10mm along the X axis' },
      { icon: Expand,          label: 'Offset',   prompt: 'Offset the solid outward by 1mm' },
    ],
  },
  {
    label: 'Boolean',
    tools: [
      { icon: Scissors,        label: 'Cut',      prompt: 'Subtract (cut) the second body from the first body' },
      { icon: GitMerge,        label: 'Fuse',     prompt: 'Fuse (union) all bodies into a single solid' },
      { icon: Target,          label: 'Hole',     prompt: 'Drill a 10mm diameter through-hole through the center of the part' },
      { icon: Nut,             label: 'Thread',   prompt: 'Add an M8 tapped through-hole at the center of the part' },
      { icon: Blend,           label: 'Intersect',prompt: 'Keep only the intersection of the current solid with a 20mm cylinder' },
    ],
  },
]

// ── Props ──────────────────────────────────────────────────────────────

interface ToolbarProps {
  showJson: boolean
  onToggleJson: () => void
  chatOpen: boolean
  onToggleChat: () => void
}

// ── Component ──────────────────────────────────────────────────────────

export function Toolbar({ showJson, onToggleJson, chatOpen, onToggleChat }: ToolbarProps) {
  const sendChatMessage = useCadStore((s) => s.sendChatMessage)
  const isChatLoading   = useCadStore((s) => s.isChatLoading)
  const isRunning       = useCadStore((s) => s.isRunning)
  const irCode          = useCadStore((s) => s.irCode)
  const runGeometry     = useCadStore((s) => s.runGeometry)
  const clearError      = useCadStore((s) => s.clearError)
  const outlinerOpen    = useCadStore((s) => s.outlinerOpen)
  const setOutlinerOpen = useCadStore((s) => s.setOutlinerOpen)

  const busy = isChatLoading || isRunning

  return (
    <header className="flex-shrink-0 bg-panel border-b border-border">

      {/* ── Top bar ──────────────────────────────────────────────────── */}
      <div className="flex items-center h-10 px-3 gap-1.5">

        {/* Logo */}
        <div className="flex items-center gap-2 mr-2">
          <div className="w-6 h-6 rounded-md bg-accent flex items-center justify-center shadow-glow">
            <Box size={13} className="text-white" strokeWidth={2.5} />
          </div>
          <div className="flex items-baseline gap-px">
            <span className="text-[13px] font-bold text-white tracking-tight leading-none">Agent</span>
            <span className="text-[13px] font-bold text-accent tracking-tight leading-none">CAD</span>
          </div>
          <span className="text-[9px] text-dim border border-divide rounded px-1 py-0.5 font-mono leading-none select-none">
            β
          </span>
        </div>

        <div className="h-5 w-px bg-border" />

        {/* Nav menus (decorative — future dropdowns) */}
        {(['File', 'Edit', 'Insert', 'Modify', 'Tools', 'View'] as const).map((label) => (
          <button
            key={label}
            className="hidden lg:flex items-center gap-0.5 px-2 py-1 rounded text-[11px] text-muted
                       hover:text-gray-200 hover:bg-raised transition-colors"
          >
            {label}
          </button>
        ))}

        <div className="flex-1" />

        {/* Computing status */}
        {(isRunning || isChatLoading) && (
          <div className="hidden sm:flex items-center gap-1.5 px-2.5 py-1 rounded-full
                          bg-accent/10 border border-accent/20 text-[10px] text-accent mr-1">
            <Loader2 size={9} className="animate-spin" />
            {isRunning ? 'Computing…' : 'Generating…'}
          </div>
        )}

        {/* Panel toggles grouped */}
        <div className="flex items-center gap-0.5 p-0.5 bg-surface rounded-lg border border-border">
          <PanelToggle
            icon={<PanelLeft size={13} />}
            active={outlinerOpen}
            title={outlinerOpen ? 'Hide model tree' : 'Show model tree'}
            onClick={() => setOutlinerOpen(!outlinerOpen)}
          />
          <PanelToggle
            icon={<MessageSquare size={13} />}
            active={chatOpen}
            title={chatOpen ? 'Hide AI chat' : 'Show AI chat'}
            onClick={onToggleChat}
          />
          <PanelToggle
            icon={<><Code2 size={11} /><span className="text-[10px] font-medium ml-0.5">JSON</span></>}
            active={showJson}
            title={showJson ? 'Hide JSON editor' : 'Show JSON editor'}
            onClick={onToggleJson}
            wide
          />
        </div>

        <div className="h-5 w-px bg-border mx-1" />

        {/* Run button */}
        <button
          onClick={() => { clearError(); runGeometry() }}
          disabled={isRunning || !irCode.trim()}
          className="flex items-center gap-1.5 px-3.5 py-1.5 rounded-md text-xs font-semibold
                     bg-accent text-white hover:bg-accent-lite disabled:opacity-25
                     disabled:cursor-not-allowed transition-all shadow-sm"
        >
          {isRunning
            ? <Loader2 size={11} className="animate-spin" />
            : <Play size={11} className="fill-current" />
          }
          Run
        </button>

        <ExportMenu />
      </div>

      {/* ── Ribbon ───────────────────────────────────────────────────── */}
      <div className="flex items-stretch h-[52px] border-t border-divide overflow-x-auto">

        {/* Tool groups */}
        {TOOL_GROUPS.map((group, gi) => (
          <div key={group.label} className="flex items-stretch">
            {gi > 0 && (
              <div className="w-px bg-divide self-stretch my-1.5 mx-0.5" />
            )}
            <ToolGroupSection
              group={group}
              disabled={busy}
              onTool={(prompt) => sendChatMessage(prompt)}
            />
          </div>
        ))}

        {/* Flexible space */}
        <div className="flex-1" />

        {/* Settings */}
        <div className="flex items-center px-2">
          <button
            className="p-1.5 rounded text-muted hover:text-gray-200 hover:bg-raised transition-colors"
            title="Settings"
          >
            <Settings size={14} />
          </button>
        </div>
      </div>
    </header>
  )
}

// ── Tool group section ─────────────────────────────────────────────────

function ToolGroupSection({
  group,
  disabled,
  onTool,
}: {
  group: ToolGroup
  disabled: boolean
  onTool: (prompt: string) => void
}) {
  return (
    <div className="flex flex-col justify-between px-1 py-1">
      <div className="flex items-center gap-0.5">
        {group.tools.map((tool) => (
          <ToolBtn
            key={tool.label}
            icon={tool.icon}
            label={tool.label}
            disabled={disabled}
            onClick={() => onTool(tool.prompt)}
          />
        ))}
      </div>
      <div className="ribbon-label text-center px-1">{group.label}</div>
    </div>
  )
}

// ── Individual tool button ─────────────────────────────────────────────

function ToolBtn({
  icon: Icon,
  label,
  disabled,
  onClick,
}: {
  icon: LucideIcon
  label: string
  disabled: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      title={label}
      className="flex flex-col items-center justify-center gap-0.5 w-11 h-9 rounded
                 hover:bg-raised active:bg-border
                 disabled:opacity-25 disabled:cursor-not-allowed
                 transition-colors group"
    >
      <Icon
        size={16}
        className="text-muted group-hover:text-gray-200 transition-colors"
        strokeWidth={1.5}
      />
      <span className="text-[9px] text-dim group-hover:text-muted transition-colors leading-none whitespace-nowrap">
        {label}
      </span>
    </button>
  )
}

// ── Panel toggle button ────────────────────────────────────────────────

function PanelToggle({
  icon,
  active,
  title,
  onClick,
  wide = false,
}: {
  icon: React.ReactNode
  active: boolean
  title: string
  onClick: () => void
  wide?: boolean
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      className={`flex items-center gap-0.5 rounded transition-colors
        ${wide ? 'px-2' : 'p-1.5'}
        ${active
          ? 'bg-accent/20 text-accent'
          : 'text-muted hover:text-gray-200 hover:bg-raised'
        }`}
    >
      {icon}
    </button>
  )
}
