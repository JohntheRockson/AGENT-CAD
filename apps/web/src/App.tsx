import { Code2 } from 'lucide-react'
import { ChatPanel }     from './components/ChatPanel'
import { EditorPanel }   from './components/EditorPanel'
import { ViewportPanel } from './components/ViewportPanel'
import { ExportMenu }    from './components/ExportMenu'
import { useCadStore }   from './store/useStore'

export default function App() {
  const showJson    = useCadStore((s) => s.showJson)
  const setShowJson = useCadStore((s) => s.setShowJson)

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden bg-surface text-gray-100 select-none">
      <header className="flex items-center gap-3 px-4 h-10 bg-panel border-b border-border flex-shrink-0 z-20">
        <span className="text-sm font-bold text-accent tracking-tight">AgentCAD</span>
        <div className="flex-1" />
        <button
          onClick={() => setShowJson(!showJson)}
          className={`flex items-center gap-1.5 px-2.5 py-1 rounded text-xs border transition-colors
            ${showJson
              ? 'border-accent/50 bg-accent/15 text-accent'
              : 'border-border text-muted hover:text-gray-200 hover:border-accent/40'}`}
        >
          <Code2 size={12} />
          JSON
        </button>
        <ExportMenu />
      </header>

      <main className="flex flex-1 min-h-0 overflow-hidden">
        <section className="flex-1 min-w-0 border-r border-border flex flex-col bg-panel">
          <ChatPanel />
        </section>

        <section className="flex-1 min-w-0 flex flex-col">
          <ViewportPanel />
        </section>

        {showJson && (
          <section className="w-[min(440px,36%)] flex-shrink-0 border-l border-border flex flex-col min-w-0">
            <EditorPanel />
          </section>
        )}
      </main>
    </div>
  )
}
