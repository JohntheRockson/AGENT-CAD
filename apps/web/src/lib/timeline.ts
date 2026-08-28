import type { BodyInstance, DocumentSnapshot, MeshData, MetricsData, TimelineSource } from '../types/cad'

export function cloneMeshData(mesh: MeshData): MeshData {
  return {
    positions: mesh.positions.slice(),
    normals:   mesh.normals.slice(),
    indices:   mesh.indices.slice(),
  }
}

export function cloneSnapshotState(state: {
  irCode: string
  bodies: BodyInstance[]
  meshData: MeshData | null
  metrics: MetricsData | null
}): Pick<DocumentSnapshot, 'irCode' | 'bodies' | 'meshData' | 'metrics'> {
  return {
    irCode: state.irCode,
    bodies: state.bodies.map((b) => ({
      ...b,
      mesh: cloneMeshData(b.mesh),
      metrics: { ...b.metrics },
    })),
    meshData: state.meshData ? cloneMeshData(state.meshData) : null,
    metrics:  state.metrics ? { ...state.metrics, bbox: [...state.metrics.bbox] as MetricsData['bbox'] } : null,
  }
}

export function makeSnapshot(
  label: string,
  source: TimelineSource,
  state: {
    irCode: string
    bodies: BodyInstance[]
    meshData: MeshData | null
    metrics: MetricsData | null
  },
): DocumentSnapshot {
  return {
    id:        crypto.randomUUID(),
    label,
    source,
    timestamp: Date.now(),
    ...cloneSnapshotState(state),
  }
}

export function truncateTimelineLabel(text: string, max = 42): string {
  const one = text.replace(/\s+/g, ' ').trim()
  if (one.length <= max) return one
  return `${one.slice(0, max - 1)}…`
}
