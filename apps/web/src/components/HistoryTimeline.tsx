import { useMemo } from 'react'
import { Clock, GitBranch } from 'lucide-react'
import { useCadStore } from '../store/useStore'

function formatTime(ts: number) {
  return new Date(ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
}

export function HistoryTimeline() {
  const timeline          = useCadStore((s) => s.timeline)
  const timelineIndex     = useCadStore((s) => s.timelineIndex)
  const restoreTimelineIndex = useCadStore((s) => s.restoreTimelineIndex)

  const current = timeline[timelineIndex]
  const atTip = timeline.length === 0 || timelineIndex >= timeline.length - 1

  const sourceLabel = useMemo(() => {
    if (!current) return ''
    switch (current.source) {
      case 'agent':     return 'Agent'
      case 'parameter': return 'Parameter'
      case 'manual':    return 'Manual'
      case 'restore':   return 'Restore'
      default:          return ''
    }
  }, [current])

  if (timeline.length === 0) {
    return null
  }

  return (
    <div className="absolute bottom-0 left-0 right-0 z-20 border-t border-border bg-panel/95 backdrop-blur-sm">
      <div className="flex items-center gap-3 px-3 py-2 min-h-[52px]">
        <div className="flex items-center gap-1.5 text-muted flex-shrink-0">
          <Clock size={13} className="text-accent/80" />
          <span className="text-[10px] font-semibold uppercase tracking-wide text-gray-400">
            History
          </span>
        </div>

        <div className="flex-1 min-w-0 flex flex-col gap-1">
          <input
            type="range"
            min={0}
            max={Math.max(0, timeline.length - 1)}
            step={1}
            value={timelineIndex}
            onChange={(e) => restoreTimelineIndex(Number.parseInt(e.target.value, 10))}
            className="w-full h-1.5 accent-accent cursor-pointer"
            aria-label="Design history timeline"
          />
          <div className="flex items-center justify-between gap-2 text-[10px] min-w-0">
            <span className="text-muted tabular-nums flex-shrink-0">
              {timelineIndex + 1} / {timeline.length}
            </span>
            <span className="truncate text-gray-300" title={current?.label}>
              {current?.label ?? '—'}
            </span>
            <span className="text-muted flex-shrink-0 hidden sm:inline">
              {sourceLabel}{current ? ` · ${formatTime(current.timestamp)}` : ''}
            </span>
          </div>
        </div>

        {!atTip && (
          <div
            className="flex items-center gap-1 px-2 py-1 rounded border border-yellow-500/30
                       bg-yellow-500/10 text-[10px] text-yellow-400/90 flex-shrink-0"
            title="You are viewing a past step. Chat and parameter edits branch from here."
          >
            <GitBranch size={11} />
            <span className="hidden md:inline">Past step</span>
          </div>
        )}
      </div>
    </div>
  )
}
