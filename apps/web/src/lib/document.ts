import type { CadBody, CadDocument, CadProgram, Feature } from '../types/cad'

export const BODY_COLORS = [
  '#4a90e2',
  '#3ecf8e',
  '#e0a458',
  '#c678dd',
  '#e06c75',
  '#56b6c2',
]

export function bodyColor(index: number): string {
  return BODY_COLORS[index % BODY_COLORS.length]
}

export function isDocument(value: unknown): value is CadDocument {
  return !!value && typeof value === 'object' && Array.isArray((value as CadDocument).bodies)
}

export function isProgram(value: unknown): value is CadProgram {
  return !!value && typeof value === 'object' && Array.isArray((value as CadProgram).features)
}

export function programToDocument(program: CadProgram): CadDocument {
  return {
    documentId: 'document',
    units: program.units ?? 'mm',
    bodies: [
      {
        bodyId: 'body_main',
        name: 'Body',
        visible: true,
        suppressed: false,
        transform: { position: [0, 0, 0], rotation: [0, 0, 0] },
        features: program.features,
        references: [],
      },
    ],
  }
}

export function parseScene(raw: unknown): CadDocument {
  if (isDocument(raw)) {
    return {
      documentId: raw.documentId || 'document',
      units: raw.units ?? 'mm',
      parameters: raw.parameters,
      bodies: raw.bodies.map(normalizeBody),
    }
  }
  if (isProgram(raw)) {
    return programToDocument(raw)
  }
  throw new Error('JSON must be a CadDocument ({ bodies }) or CadProgram ({ features })')
}

function normalizeBody(body: CadBody, index: number): CadBody {
  return {
    bodyId: body.bodyId || `body_${index + 1}`,
    name: body.name || body.bodyId || `Body ${index + 1}`,
    visible: body.visible !== false,
    suppressed: !!body.suppressed,
    transform: {
      position: body.transform?.position ?? [0, 0, 0],
      rotation: body.transform?.rotation ?? [0, 0, 0],
    },
    features: body.features ?? [],
    references: body.references ?? [],
  }
}

export function parseSceneJson(text: string): CadDocument {
  return parseScene(JSON.parse(text) as unknown)
}

export function setDocumentParameter(
  doc: CadDocument,
  name: string,
  value: number,
): CadDocument {
  const old = doc.parameters?.[name]
  let next: CadDocument = {
    ...doc,
    parameters: {
      ...(doc.parameters ?? {}),
      [name]: value,
    },
  }
  if (old != null && Number.isFinite(old) && Math.abs(old - value) > 1e-12) {
    const replaced = { v: false }
    next = rewriteNumbers(next, old, value, undefined, replaced) as CadDocument
    next.parameters = { ...(doc.parameters ?? {}), [name]: value }
    if (isAxialOverallName(name) && !replaced.v) {
      const target = maxAxial(next)
      if (target > 0.05) {
        next = bumpLargestAxial(next, target, value - old) as CadDocument
        next.parameters = { ...(doc.parameters ?? {}), [name]: value }
      }
    }
  }
  return next
}

const AXIAL_KEYS = new Set(['length', 'depth', 'height'])

/** Scale numbers that match common ratios of the old parameter (hex vertices, halves). */
function scaleLike(v: number, oldVal: number, newVal: number, key?: string): number | null {
  const tol = Math.max(Math.abs(oldVal) * 0.002, 0.02)
  const axial = key === 'length' || key === 'depth' || key === 'height'
  const ratios = axial
    ? [1, 0.5]
    : [1, 0.5, 1 / Math.sqrt(3), Math.sqrt(3) / 2, 2 / Math.sqrt(3), 0.25]
  const sign = v < 0 ? -1 : 1
  const mag = Math.abs(v)
  for (const r of ratios) {
    if (Math.abs(mag - Math.abs(oldVal) * r) <= Math.max(tol, Math.abs(oldVal) * r * 0.002)) {
      return sign * Math.abs(newVal) * r
    }
  }
  return null
}

function rewriteNumbers(node: unknown, oldVal: number, newVal: number, key?: string, replaced?: { v: boolean }): unknown {
  if (node && typeof node === 'object') {
    if (Array.isArray(node)) {
      return node.map((item) => rewriteNumbers(item, oldVal, newVal, key, replaced))
    }
    const rec = node as Record<string, unknown>
    const out: Record<string, unknown> = {}
    for (const [k, v] of Object.entries(rec)) {
      if (k === 'parameters') {
        out[k] = v
        continue
      }
      out[k] = rewriteNumbers(v, oldVal, newVal, k, replaced)
    }
    return out
  }
  if (typeof node === 'number') {
    if (key && ['op', 'bodyId', 'documentId', 'name', 'units', 'plane', 'axis', 'kind', 'id', 'hand'].includes(key)) {
      return node
    }
    const nv = scaleLike(node, oldVal, newVal, key)
    if (nv != null) {
      if (Math.abs(node - oldVal) <= Math.max(Math.abs(oldVal) * 0.002, 0.02)) {
        if (replaced) replaced.v = true
      }
      return nv
    }
  }
  return node
}

function bumpLargestAxial(node: unknown, target: number, delta: number, key?: string): unknown {
  if (node && typeof node === 'object') {
    if (Array.isArray(node)) {
      return node.map((item) => bumpLargestAxial(item, target, delta, key))
    }
    const rec = node as Record<string, unknown>
    const out: Record<string, unknown> = {}
    for (const [k, v] of Object.entries(rec)) {
      if (k === 'parameters') {
        out[k] = v
        continue
      }
      out[k] = bumpLargestAxial(v, target, delta, k)
    }
    return out
  }
  if (typeof node === 'number' && key && AXIAL_KEYS.has(key) && Math.abs(node - target) < 1e-9) {
    return Math.max(0.05, node + delta)
  }
  return node
}

function maxAxial(node: unknown, key?: string): number {
  let best = 0
  const walk = (n: unknown, k?: string) => {
    if (n && typeof n === 'object') {
      if (Array.isArray(n)) {
        n.forEach((item) => walk(item, k))
        return
      }
      for (const [ck, v] of Object.entries(n as Record<string, unknown>)) {
        if (ck === 'parameters') continue
        walk(v, ck)
      }
      return
    }
    if (typeof n === 'number' && k && AXIAL_KEYS.has(k) && n > best) best = n
  }
  walk(node, key)
  return best
}

function isAxialOverallName(name: string): boolean {
  const n = name.toLowerCase()
  return (
    n === 'length' ||
    n === 'height' ||
    n === 'bolt_length' ||
    n === 'overall_length' ||
    n === 'total_length' ||
    (n.endsWith('_length') && !n.includes('head') && !n.includes('pitch'))
  )
}

export function parameterEntries(doc: CadDocument): Array<[string, number]> {
  const p = doc.parameters ?? {}
  return Object.entries(p).sort(([a], [b]) => a.localeCompare(b))
}

export function formatParameterName(name: string): string {
  return name.replace(/_/g, ' ')
}

export function unitSuffix(units: CadDocument['units']): string {
  return units === 'in' ? 'in' : 'mm'
}

export function prettyDocument(doc: CadDocument): string {
  return JSON.stringify(doc, null, 2)
}

export function featureLabel(feature: Feature, index: number): string {
  switch (feature.op) {
    case 'box':
      return `Box ${feature.size.join('×')}`
    case 'cylinder':
      return `Cylinder Ø${feature.diameter}×${feature.height}`
    case 'sphere':
      return `Sphere Ø${feature.diameter}`
    case 'cone':
      return `Cone Ø${feature.d1}→${feature.d2}`
    case 'extrude':
      return `Extrude ${feature.depth}`
    case 'revolve':
      return `Revolve ${feature.axis}`
    case 'hole':
      return `Hole Ø${feature.diameter}`
    case 'thread':
      return `Thread ${feature.size ?? feature.kind}`
    case 'helix':
      return `Helix r${feature.radius}`
    case 'ellipsoid':
      return `Ellipsoid ${feature.radii.join('×')}`
    case 'sweep':
      return 'Sweep'
    case 'offset':
      return `Offset ${feature.distance}`
    case 'thicken':
      return `Thicken ${feature.thickness}`
    case 'common':
      return 'Intersect'
    case 'draft':
      return `Draft ${feature.angle}°`
    case 'cut':
      return 'Cut'
    case 'fuse':
      return 'Fuse'
    case 'fillet':
      return `Fillet r${feature.radius}`
    case 'chamfer':
      return `Chamfer ${feature.distance}`
    case 'sketch':
      return 'Sketch'
    default:
      return feature.op.replace(/_/g, ' ')
  }
}

export function featureKey(feature: Feature, index: number): string {
  if ('id' in feature && typeof feature.id === 'string' && feature.id) {
    return feature.id
  }
  return `${feature.op}_${index}`
}
