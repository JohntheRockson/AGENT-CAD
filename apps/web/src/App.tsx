import { useState } from 'react'
import { ChatPanel }     from './components/ChatPanel'
import { EditorPanel }   from './components/EditorPanel'
import { ViewportPanel } from './components/ViewportPanel'
import { Toolbar }       from './components/Toolbar'
import { StatusBar }     from './components/StatusBar'
import { Outliner }      from './components/Outliner'
import { useCadStore }   from './store/useStore'

export default function App() {
  const showJson       = useCadStore((s) => s.showJson)
  const setShowJson    = useCadStore((s) => s.setShowJson)
  const outlinerOpen   = useCadStore((s) => s.outlinerOpen)
  const [chatOpen, setChatOpen] = useState(true)

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden bg-surface text-gray-100">

      {/* ── Toolbar / Ribbon ──────────────────────────────────────── */}
      <Toolbar
        showJson={showJson}
        onToggleJson={() => setShowJson(!showJson)}
        chatOpen={chatOpen}
        onToggleChat={() => setChatOpen((v) => !v)}
      />

      {/* ── Main 3-panel area ─────────────────────────────────────── */}
      <main className="flex flex-1 min-h-0 overflow-hidden">

        {/* Left: Model tree / Outliner */}
        {outlinerOpen && (
          <aside className="w-52 flex-shrink-0 border-r border-border flex flex-col bg-panel min-h-0">
            <Outliner />
          </aside>
        )}

        {/* Center: 3-D Viewport */}
        <section className="flex-1 min-w-0 flex flex-col min-h-0">
          <ViewportPanel />
        </section>

        {/* Right: AI Chat */}
        {chatOpen && (
          <aside className="w-72 flex-shrink-0 border-l border-border flex flex-col bg-panel min-h-0">
            <ChatPanel />
          </aside>
        )}

        {/* Optional: JSON editor */}
        {showJson && (
          <aside className="w-[min(440px,35%)] flex-shrink-0 border-l border-border flex flex-col min-w-0 min-h-0">
            <EditorPanel />
          </aside>
        )}
      </main>

      {/* ── Status bar ────────────────────────────────────────────── */}
      <StatusBar />
    </div>
  )
}
