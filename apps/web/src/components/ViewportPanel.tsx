import { useEffect, useMemo, useRef } from 'react'
import { Canvas, useThree } from '@react-three/fiber'
import { OrbitControls, GizmoHelper, GizmoViewport } from '@react-three/drei'
import * as THREE from 'three'
import { Loader2, Box } from 'lucide-react'
import { useCadStore } from '../store/useStore'
import { bodyColor } from '../lib/document'
import { Outliner } from './Outliner'
import { ParametersPanel } from './ParametersPanel'
import { HistoryTimeline } from './HistoryTimeline'
import type { BodyInstance, MeshData, MetricsData } from '../types/cad'

// ── CAD mesh with edge overlay ────────────────────────────────────────────────

function CadMesh({
  mesh,
  color,
  selected,
  hovered,
  onSelect,
  onHover,
}: {
  mesh: MeshData
  color: string
  selected: boolean
  hovered: boolean
  onSelect: () => void
  onHover: (h: boolean) => void
}) {
  const geometry = useMemo(() => {
    const geo = new THREE.BufferGeometry()

    geo.setAttribute(
      'position',
      new THREE.Float32BufferAttribute(new Float32Array(mesh.positions), 3),
    )

    if (mesh.indices.length > 0) {
      geo.setIndex(new THREE.Uint32BufferAttribute(new Uint32Array(mesh.indices), 1))
    }

    if (mesh.normals.length === mesh.positions.length && mesh.normals.length > 0) {
      geo.setAttribute(
        'normal',
        new THREE.Float32BufferAttribute(new Float32Array(mesh.normals), 3),
      )
    } else {
      geo.computeVertexNormals()
    }
    geo.computeBoundingBox()
    geo.computeBoundingSphere()

    return geo
  }, [mesh])

  const edgeGeo = useMemo(() => new THREE.EdgesGeometry(geometry, 35), [geometry])
  const tint = selected ? new THREE.Color(color).lerp(new THREE.Color('#ffffff'), 0.28) : new THREE.Color(color)

  return (
    <group>
      <mesh
        geometry={geometry}
        castShadow
        receiveShadow
        onClick={(e) => {
          e.stopPropagation()
          if (e.delta > 4) return
          onSelect()
        }}
        onPointerOver={(e) => {
          e.stopPropagation()
          onHover(true)
          document.body.style.cursor = 'pointer'
        }}
        onPointerOut={() => {
          onHover(false)
          document.body.style.cursor = 'auto'
        }}
      >
        <meshPhongMaterial
          color={tint}
          emissive={selected || hovered ? new THREE.Color(color) : new THREE.Color(0x000000)}
          emissiveIntensity={selected ? 0.35 : hovered ? 0.18 : 0}
          shininess={40}
          specular={new THREE.Color(0x223344)}
          side={THREE.DoubleSide}
          polygonOffset
          polygonOffsetFactor={1}
          polygonOffsetUnits={1}
        />
      </mesh>

      <lineSegments geometry={edgeGeo} renderOrder={1} raycast={() => {}}>
        <lineBasicMaterial
          color={selected ? new THREE.Color('#f0f4ff') : new THREE.Color(0x1a2a3a)}
          depthWrite={false}
        />
      </lineSegments>
    </group>
  )
}

// ── Camera auto-fit ───────────────────────────────────────────────────────────

function CameraFit({ metrics }: { metrics: MetricsData | null }) {
  const { camera } = useThree()
  const fitted = useRef(false)

  useEffect(() => {
    if (!metrics) return
    camera.up.set(0, 0, 1)
    const [xmin, ymin, zmin, xmax, ymax, zmax] = metrics.bbox
    const cx = (xmin + xmax) / 2
    const cy = (ymin + ymax) / 2
    const cz = (zmin + zmax) / 2
    const size = Math.max(xmax - xmin, ymax - ymin, zmax - zmin, 1)
    const dist = size * 2.5

    camera.position.set(cx + dist * 0.7, cy - dist * 0.9, cz + dist * 0.6)
    camera.lookAt(cx, cy, cz)
    camera.updateProjectionMatrix()
    fitted.current = true
  }, [metrics, camera])

  return null
}

// ── Grid floor ────────────────────────────────────────────────────────────────

function GridFloor({ metrics }: { metrics: MetricsData | null }) {
  const z = metrics ? metrics.bbox[2] - 0.15 : -0.01
  const grid = useMemo(() => {
    const g = new THREE.GridHelper(500, 50, 0x2a2a3a, 0x1e1e2e)
    g.rotation.x = Math.PI / 2
    return g
  }, [])
  grid.position.z = z
  return <primitive object={grid} />
}

// ── Viewport scene ────────────────────────────────────────────────────────────

function Scene({
  bodies,
  fallbackMesh,
  metrics,
}: {
  bodies: BodyInstance[]
  fallbackMesh: MeshData | null
  metrics: MetricsData | null
}) {
  const selectedBodyId  = useCadStore((s) => s.selectedBodyId)
  const hoveredBodyId   = useCadStore((s) => s.hoveredBodyId)
  const isolatedBodyId  = useCadStore((s) => s.isolatedBodyId)
  const selectBody      = useCadStore((s) => s.selectBody)
  const hoverBody       = useCadStore((s) => s.hoverBody)
  const setOutlinerOpen = useCadStore((s) => s.setOutlinerOpen)

  const instances = bodies.length
    ? bodies
    : fallbackMesh
      ? [{
          bodyId: 'body_main',
          name: 'Body',
          visible: true,
          suppressed: false,
          mesh: fallbackMesh,
          metrics: metrics ?? { volume: 0, bbox: [0, 0, 0, 1, 1, 1], surface_area: 0, is_solid: true },
        }]
      : []

  return (
    <>
      <CameraFit metrics={metrics} />
      <GridFloor metrics={metrics} />

      <ambientLight intensity={0.35} />
      <directionalLight position={[15, -20, 25]} intensity={0.9} castShadow />
      <directionalLight position={[-10, 15, -5]} intensity={0.2} />

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
            hovered={hoveredBodyId === body.bodyId}
            onSelect={() => {
              selectBody(body.bodyId)
              setOutlinerOpen(true)
            }}
            onHover={(h) => hoverBody(h ? body.bodyId : null)}
          />
        )
      })}

      <OrbitControls makeDefault enableDamping dampingFactor={0.08} />

      <GizmoHelper alignment="bottom-right" margin={[70, 70]}>
        <GizmoViewport
          axisColors={['#e06c75', '#98c379', '#61afef']}
          labelColor="white"
        />
      </GizmoHelper>
    </>
  )
}

// ── Exported component ────────────────────────────────────────────────────────

export function ViewportPanel() {
  const meshData        = useCadStore((s) => s.meshData)
  const metrics         = useCadStore((s) => s.metrics)
  const bodies          = useCadStore((s) => s.bodies)
  const isRunning       = useCadStore((s) => s.isRunning)
  const selectedBodyId  = useCadStore((s) => s.selectedBodyId)
  const isolatedBodyId  = useCadStore((s) => s.isolatedBodyId)
  const selectBody      = useCadStore((s) => s.selectBody)

  const timelineLen     = useCadStore((s) => s.timeline.length)

  const selected = bodies.find((b) => b.bodyId === selectedBodyId)
  const hasSolid = bodies.some((b) => b.visible && !b.suppressed) || !!meshData
  const pointerDown = useRef({ x: 0, y: 0 })
  const bottomPad = timelineLen > 0 ? 'pb-[58px]' : ''

  return (
    <div
      className={`relative h-full w-full bg-[#0a0e14] ${bottomPad}`}
      onPointerDown={(e) => {
        pointerDown.current = { x: e.clientX, y: e.clientY }
      }}
    >
      <div className="absolute top-0 left-0 right-0 h-9 flex items-center px-3 gap-2 z-10
                      bg-gradient-to-b from-[#0a0e14cc] to-transparent pointer-events-none">
        <Box size={13} className="text-accent" />
        <span className="text-xs font-semibold text-gray-300 tracking-wide uppercase">
          Viewport
        </span>
        {selected && (
          <span className="text-[10px] text-accent/90 truncate max-w-[40%]">
            {selected.name}
          </span>
        )}
        {isolatedBodyId && (
          <span className="text-[10px] text-muted">isolated</span>
        )}
        {isRunning && (
          <span className="flex items-center gap-1 text-[10px] text-accent ml-2">
            <Loader2 size={10} className="animate-spin" />
            Computing…
          </span>
        )}
      </div>

      <Outliner />
      <ParametersPanel />

      <Canvas
        camera={{ position: [120, -140, 90], fov: 45, up: [0, 0, 1] }}
        gl={{ antialias: true, alpha: false }}
        shadows
        style={{ background: '#0a0e14' }}
        onCreated={({ camera }) => {
          camera.up.set(0, 0, 1)
        }}
        onPointerMissed={(e) => {
          const dx = e.clientX - pointerDown.current.x
          const dy = e.clientY - pointerDown.current.y
          if (Math.hypot(dx, dy) > 5) return
          selectBody(null)
        }}
      >
        <Scene bodies={bodies} fallbackMesh={meshData} metrics={metrics} />
      </Canvas>

      {!hasSolid && !isRunning && (
        <div className="absolute inset-0 flex flex-col items-center justify-center pointer-events-none">
          <Box size={36} className="text-border mb-3" strokeWidth={1} />
          <p className="text-xs text-muted">Describe a part in chat to render it</p>
        </div>
      )}

      <HistoryTimeline />
    </div>
  )
}
