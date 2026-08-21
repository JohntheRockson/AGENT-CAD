import { useEffect, useMemo, useRef, useState } from 'react'
import { Canvas, useThree } from '@react-three/fiber'
import { OrbitControls, GizmoHelper, GizmoViewport } from '@react-three/drei'
import * as THREE from 'three'
import { Loader2, Box, Maximize2, Grid3x3, Sun, Square, Spline } from 'lucide-react'
import { useCadStore } from '../store/useStore'
import { bodyColor } from '../lib/document'
import type { BodyInstance, MeshData, MetricsData } from '../types/cad'

// ── Types ─────────────────────────────────────────────────────────────

type DisplayMode = 'shaded' | 'wireframe' | 'edges'
type CameraViewId = 'iso' | 'top' | 'front' | 'right' | 'left' | 'bottom'
interface CameraRequest { view: CameraViewId; id: number }

// ── Camera view presets ───────────────────────────────────────────────

const CAM_VIEWS: { id: CameraViewId; label: string }[] = [
  { id: 'iso',    label: 'ISO' },
  { id: 'top',    label: 'TOP' },
  { id: 'front',  label: 'FRT' },
  { id: 'right',  label: 'RGT' },
  { id: 'left',   label: 'LFT' },
]

// ── CAD mesh ──────────────────────────────────────────────────────────

function CadMesh({
  mesh, color, selected, hovered, displayMode,
  onSelect, onHover,
}: {
  mesh: MeshData
  color: string
  selected: boolean
  hovered: boolean
  displayMode: DisplayMode
  onSelect: () => void
  onHover: (h: boolean) => void
}) {
  const geometry = useMemo(() => {
    const geo = new THREE.BufferGeometry()
    geo.setAttribute('position', new THREE.Float32BufferAttribute(new Float32Array(mesh.positions), 3))
    if (mesh.indices.length > 0)
      geo.setIndex(new THREE.Uint32BufferAttribute(new Uint32Array(mesh.indices), 1))
    if (mesh.normals.length === mesh.positions.length && mesh.normals.length > 0)
      geo.setAttribute('normal', new THREE.Float32BufferAttribute(new Float32Array(mesh.normals), 3))
    else
      geo.computeVertexNormals()
    geo.computeBoundingBox()
    geo.computeBoundingSphere()
    return geo
  }, [mesh])

  const edgeGeo = useMemo(() => new THREE.EdgesGeometry(geometry, 30), [geometry])

  const tint = selected
    ? new THREE.Color(color).lerp(new THREE.Color('#ffffff'), 0.3)
    : new THREE.Color(color)

  const showFaces = displayMode !== 'edges'
  const showEdges = displayMode !== 'shaded' || selected

  return (
    <group>
      {showFaces && (
        <mesh
          geometry={geometry}
          castShadow
          receiveShadow
          onClick={(e) => { e.stopPropagation(); if (e.delta > 4) return; onSelect() }}
          onPointerOver={(e) => { e.stopPropagation(); onHover(true); document.body.style.cursor = 'pointer' }}
          onPointerOut={() => { onHover(false); document.body.style.cursor = 'auto' }}
        >
          <meshPhongMaterial
            color={tint}
            emissive={selected || hovered ? new THREE.Color(color) : new THREE.Color(0x000000)}
            emissiveIntensity={selected ? 0.35 : hovered ? 0.18 : 0}
            wireframe={displayMode === 'wireframe'}
            shininess={40}
            specular={new THREE.Color(0x223344)}
            side={THREE.DoubleSide}
            polygonOffset
            polygonOffsetFactor={1}
            polygonOffsetUnits={1}
          />
        </mesh>
      )}

      {showEdges && (
        <lineSegments geometry={edgeGeo} renderOrder={1} raycast={() => {}}>
          <lineBasicMaterial
            color={selected ? '#e0eaff' : displayMode === 'edges' ? '#6080c0' : '#1a2a3a'}
            depthWrite={false}
          />
        </lineSegments>
      )}
    </group>
  )
}

// ── Camera auto-fit ───────────────────────────────────────────────────

function CameraFit({ metrics }: { metrics: MetricsData | null }) {
  const { camera } = useThree()
  const fitted = useRef(false)

  useEffect(() => {
    if (!metrics || fitted.current) return
    fitted.current = true
    positionCamera(camera, 'iso', metrics)
  }, [metrics, camera])

  return null
}

// ── Camera controller (responds to view requests) ─────────────────────

function CameraViewController({
  req,
  orbitRef,
  metrics,
}: {
  req: CameraRequest | null
  orbitRef: React.RefObject<any>
  metrics: MetricsData | null
}) {
  const { camera } = useThree()
  const prevId = useRef(-1)

  useEffect(() => {
    if (!req || req.id === prevId.current) return
    prevId.current = req.id

    positionCamera(camera, req.view, metrics)

    if (orbitRef.current) {
      const c = getCenter(metrics)
      orbitRef.current.target.set(c[0], c[1], c[2])
      orbitRef.current.update()
    }
  }, [req?.id])

  return null
}

function getCenter(metrics: MetricsData | null): [number, number, number] {
  if (!metrics) return [0, 0, 0]
  const [xmin, ymin, zmin, xmax, ymax, zmax] = metrics.bbox
  return [(xmin + xmax) / 2, (ymin + ymax) / 2, (zmin + zmax) / 2]
}

function positionCamera(
  camera: THREE.Camera,
  view: CameraViewId,
  metrics: MetricsData | null,
) {
  const [xmin, ymin, zmin, xmax, ymax, zmax] = metrics?.bbox ?? [0, 0, 0, 100, 100, 100]
  const cx = (xmin + xmax) / 2
  const cy = (ymin + ymax) / 2
  const cz = (zmin + zmax) / 2
  const size = Math.max(xmax - xmin, ymax - ymin, zmax - zmin, 50)
  const d = size * 2.8
  camera.up.set(0, 0, 1)

  switch (view) {
    case 'top':    camera.position.set(cx,        cy,        cz + d); break
    case 'bottom': camera.position.set(cx,        cy,        cz - d); break
    case 'front':  camera.position.set(cx,        cy - d,   cz + size * 0.3); break
    case 'right':  camera.position.set(cx + d,    cy,        cz + size * 0.3); break
    case 'left':   camera.position.set(cx - d,    cy,        cz + size * 0.3); break
    default:       camera.position.set(cx + d * 0.7, cy - d * 0.9, cz + d * 0.6)
  }

  camera.lookAt(cx, cy, cz)
  if (camera instanceof THREE.PerspectiveCamera) {
    camera.near = Math.max(size / 10_000, 0.01)
    camera.far  = Math.max(size * 400, d * 80, 50_000)
    camera.updateProjectionMatrix()
  }
}

// ── Grid floor ────────────────────────────────────────────────────────

function GridFloor({ metrics, visible }: { metrics: MetricsData | null; visible: boolean }) {
  if (!visible) return null
  const z    = metrics ? metrics.bbox[2] - 0.15 : -0.01
  const span = metrics
    ? Math.max((metrics.bbox[3] - metrics.bbox[0]) * 4, (metrics.bbox[4] - metrics.bbox[1]) * 4, 500)
    : 500
  const grid = useMemo(() => {
    const divs = Math.min(100, Math.max(20, Math.round(span / 20)))
    const g = new THREE.GridHelper(span, divs, 0x252538, 0x1a1a28)
    g.rotation.x = Math.PI / 2
    return g
  }, [span])
  grid.position.z = z
  return <primitive object={grid} />
}

// ── Scene ─────────────────────────────────────────────────────────────

function Scene({
  bodies, fallbackMesh, metrics,
  displayMode, cameraReq, showGrid,
}: {
  bodies: BodyInstance[]
  fallbackMesh: MeshData | null
  metrics: MetricsData | null
  displayMode: DisplayMode
  cameraReq: CameraRequest | null
  showGrid: boolean
}) {
  const selectedBodyId  = useCadStore((s) => s.selectedBodyId)
  const hoveredBodyId   = useCadStore((s) => s.hoveredBodyId)
  const isolatedBodyId  = useCadStore((s) => s.isolatedBodyId)
  const selectBody      = useCadStore((s) => s.selectBody)
  const hoverBody       = useCadStore((s) => s.hoverBody)

  const orbitRef = useRef<any>(null)

  const instances = bodies.length
    ? bodies
    : fallbackMesh
      ? [{ bodyId: 'body_main', name: 'Body', visible: true, suppressed: false, mesh: fallbackMesh,
           metrics: metrics ?? { volume: 0, bbox: [0,0,0,1,1,1] as [number,number,number,number,number,number], surface_area: 0, is_solid: true } }]
      : []

  return (
    <>
      <CameraFit metrics={metrics} />
      <CameraViewController req={cameraReq} orbitRef={orbitRef} metrics={metrics} />
      <GridFloor metrics={metrics} visible={showGrid} />

      <ambientLight intensity={0.4} />
      <directionalLight position={[15, -20, 25]} intensity={0.95} castShadow />
      <directionalLight position={[-10, 15, -5]} intensity={0.25} />
      <directionalLight position={[0, 0, -15]}   intensity={0.15} />

      {instances.map((body, index) => {
        const shown = isolatedBodyId
          ? body.bodyId === isolatedBodyId
          : body.visible && !body.suppressed
        if (!shown) return null
        return (
          <CadMesh
            key={body.bodyId}
            mesh={body.mesh}
            color={bodyColor(index)}
            selected={selectedBodyId === body.bodyId}
            hovered={hoveredBodyId  === body.bodyId}
            displayMode={displayMode}
            onSelect={() => selectBody(body.bodyId)}
            onHover={(h) => hoverBody(h ? body.bodyId : null)}
          />
        )
      })}

      <OrbitControls
        ref={orbitRef}
        makeDefault
        enableDamping
        dampingFactor={0.08}
        minDistance={0.05}
        maxDistance={1e8}
      />

      <GizmoHelper alignment="bottom-right" margin={[72, 72]}>
        <GizmoViewport
          axisColors={['#e06c75', '#98c379', '#61afef']}
          labelColor="white"
        />
      </GizmoHelper>
    </>
  )
}

// ── Exported component ────────────────────────────────────────────────

export function ViewportPanel() {
  const meshData       = useCadStore((s) => s.meshData)
  const metrics        = useCadStore((s) => s.metrics)
  const bodies         = useCadStore((s) => s.bodies)
  const isRunning      = useCadStore((s) => s.isRunning)
  const selectedBodyId = useCadStore((s) => s.selectedBodyId)
  const isolatedBodyId = useCadStore((s) => s.isolatedBodyId)
  const selectBody     = useCadStore((s) => s.selectBody)

  const [displayMode, setDisplayMode] = useState<DisplayMode>('shaded')
  const [cameraReq, setCameraReq]     = useState<CameraRequest | null>(null)
  const [showGrid, setShowGrid]       = useState(true)
  const [activeView, setActiveView]   = useState<CameraViewId>('iso')

  const selected = bodies.find((b) => b.bodyId === selectedBodyId)
  const hasSolid = bodies.some((b) => b.visible && !b.suppressed) || !!meshData
  const pointerDown = useRef({ x: 0, y: 0 })

  const requestView = (view: CameraViewId) => {
    setActiveView(view)
    setCameraReq({ view, id: Date.now() })
  }

  return (
    <div
      className="relative h-full w-full bg-vp"
      onPointerDown={(e) => { pointerDown.current = { x: e.clientX, y: e.clientY } }}
    >
      {/* ── Top overlay bar ──────────────────────────────────────── */}
      <div className="absolute top-0 left-0 right-0 z-10 flex items-center px-2.5 h-9 gap-2
                      bg-gradient-to-b from-[#0a0a1488] to-transparent pointer-events-none">
        <Box size={12} className="text-accent" />
        <span className="text-[11px] font-semibold text-gray-400 tracking-widest uppercase">
          Viewport
        </span>
        {selected && (
          <span className="text-[10px] text-accent/80 truncate max-w-[30%]">
            — {selected.name}
          </span>
        )}
        {isolatedBodyId && (
          <span className="text-[9px] px-1.5 py-0.5 rounded bg-amber/15 text-amber border border-amber/25">
            ISOLATED
          </span>
        )}
        {isRunning && (
          <span className="flex items-center gap-1 text-[10px] text-accent">
            <Loader2 size={10} className="animate-spin" />
            Computing…
          </span>
        )}
      </div>

      {/* ── Camera view presets (top-left) ───────────────────────── */}
      <div className="absolute top-10 left-2 z-10 flex flex-col gap-0.5">
        <div className="flex flex-col rounded-md overflow-hidden border border-border bg-panel/80 backdrop-blur-sm shadow-cad">
          {CAM_VIEWS.map((v) => (
            <button
              key={v.id}
              onClick={() => requestView(v.id)}
              title={`${v.label} view`}
              className={`px-2.5 py-1 text-[10px] font-mono font-bold tracking-widest transition-colors
                          border-b border-divide last:border-b-0
                          ${activeView === v.id
                            ? 'bg-accent/20 text-accent'
                            : 'text-muted hover:bg-raised hover:text-gray-200'
                          }`}
            >
              {v.label}
            </button>
          ))}
        </div>
      </div>

      {/* ── Display + grid controls (bottom-left) ────────────────── */}
      <div className="absolute bottom-16 left-2 z-10 flex flex-col gap-1.5">
        {/* Display mode */}
        <div className="flex flex-col rounded-md overflow-hidden border border-border bg-panel/80 backdrop-blur-sm shadow-cad">
          <DisplayBtn
            icon={<Sun size={12} />}
            label="Shaded"
            active={displayMode === 'shaded'}
            onClick={() => setDisplayMode('shaded')}
          />
          <DisplayBtn
            icon={<Square size={12} />}
            label="Wire"
            active={displayMode === 'wireframe'}
            onClick={() => setDisplayMode('wireframe')}
          />
          <DisplayBtn
            icon={<Spline size={12} />}
            label="Edges"
            active={displayMode === 'edges'}
            onClick={() => setDisplayMode('edges')}
          />
        </div>

        {/* Grid toggle */}
        <button
          onClick={() => setShowGrid((v) => !v)}
          title={showGrid ? 'Hide grid' : 'Show grid'}
          className={`p-1.5 rounded-md border shadow-cad transition-colors backdrop-blur-sm
                      ${showGrid
                        ? 'border-accent/30 bg-accent/15 text-accent'
                        : 'border-border bg-panel/80 text-muted hover:text-gray-200'
                      }`}
        >
          <Grid3x3 size={13} />
        </button>

        {/* Fit to view */}
        <button
          onClick={() => requestView('iso')}
          title="Fit to view"
          className="p-1.5 rounded-md border border-border bg-panel/80 text-muted
                     hover:text-gray-200 hover:border-accent/30 shadow-cad transition-colors backdrop-blur-sm"
        >
          <Maximize2 size={13} />
        </button>
      </div>

      {/* ── Three.js canvas ──────────────────────────────────────── */}
      <Canvas
        camera={{ position: [120, -140, 90], fov: 45, up: [0, 0, 1], near: 0.05, far: 1e7 }}
        gl={{ antialias: true, alpha: false, logarithmicDepthBuffer: true }}
        shadows
        style={{ background: '#0a0a14' }}
        onCreated={({ camera }) => { camera.up.set(0, 0, 1) }}
        onPointerMissed={(e) => {
          const dx = e.clientX - pointerDown.current.x
          const dy = e.clientY - pointerDown.current.y
          if (Math.hypot(dx, dy) > 5) return
          selectBody(null)
        }}
      >
        <Scene
          bodies={bodies}
          fallbackMesh={meshData}
          metrics={metrics}
          displayMode={displayMode}
          cameraReq={cameraReq}
          showGrid={showGrid}
        />
      </Canvas>

      {/* ── Empty state ──────────────────────────────────────────── */}
      {!hasSolid && !isRunning && (
        <div className="absolute inset-0 flex flex-col items-center justify-center pointer-events-none">
          <Box size={48} className="text-border mb-4" strokeWidth={0.8} />
          <p className="text-sm text-muted font-medium">No geometry</p>
          <p className="text-[11px] text-dim mt-1">
            Describe a part in the AI chat or click a toolbar primitive
          </p>
        </div>
      )}
    </div>
  )
}

// ── Display mode button ───────────────────────────────────────────────

function DisplayBtn({
  icon, label, active, onClick,
}: {
  icon: React.ReactNode
  label: string
  active: boolean
  onClick: () => void
}) {
  return (
    <button
      onClick={onClick}
      title={`${label} display`}
      className={`flex items-center gap-1.5 px-2 py-1 text-[10px] font-medium transition-colors
                  border-b border-divide last:border-b-0
                  ${active ? 'bg-accent/20 text-accent' : 'text-muted hover:bg-raised hover:text-gray-200'}`}
    >
      {icon}
      {label}
    </button>
  )
}
