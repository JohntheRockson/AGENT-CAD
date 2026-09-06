import type { CadBody, CadDocument, CadProgram, Feature, ThreadOp } from '../types/cad'

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

const BOLT_LENGTH_NAMES = ['bolt_length', 'overall_length', 'total_length']
const HEAD_HEIGHT_NAMES = ['head_height', 'hex_height', 'head_depth']
const DEAD_HEIGHT_NAMES = [
  'dead_height',
  'dead_length',
  'unthreaded_length',
  'unthreaded_height',
]

const SKIP_REWRITE_KEYS = new Set([
  'op',
  'bodyId',
  'documentId',
  'name',
  'units',
  'plane',
  'axis',
  'kind',
  'id',
  'hand',
])

/**
 * Named parameters shown in the panel. Golden hex-bolt IR often bakes
 * `bolt_length` / `head_height` / `dead_height` as feature literals and omits
 * the parameters map — still surface those three so type-in can drive them.
 */
export function resolvedParameters(doc: CadDocument): Record<string, number> {
  const explicit = { ...(doc.parameters ?? {}) }
  const inferred = inferBoltParameters(doc)
  const out: Record<string, number> = { ...explicit }
  if (!hasNamed(out, BOLT_LENGTH_NAMES) && inferred.bolt_length != null) {
    out.bolt_length = inferred.bolt_length
  }
  if (!hasNamed(out, HEAD_HEIGHT_NAMES) && inferred.head_height != null) {
    out.head_height = inferred.head_height
  }
  if (!hasNamed(out, DEAD_HEIGHT_NAMES) && inferred.dead_height != null) {
    out.dead_height = inferred.dead_height
  }
  return out
}

export function setDocumentParameter(
  doc: CadDocument,
  name: string,
  value: number,
): CadDocument {
  const resolved = resolvedParameters(doc)
  const old = resolved[name]
  let next: CadDocument = {
    ...doc,
    parameters: {
      ...resolved,
      [name]: value,
    },
  }
  if (old == null || !Number.isFinite(old) || Math.abs(old - value) <= 1e-12) {
    return next
  }
  if (isHeadHeightName(name)) {
    next = applyHexHeadHeight(next, old, value)
    next.parameters = { ...resolved, [name]: value }
    return next
  }
  if (isDeadHeightName(name)) {
    next = applyDeadHeight(next, old, value)
    next.parameters = { ...resolved, [name]: value }
    return next
  }
  next = applyParameterDelta(next, name, old, value)
  next.parameters = { ...resolved, [name]: value }
  return next
}

/** Treat as unchanged so draft/commit guards skip a no-op rebuild. */
export function sameParameterValue(a: number, b: number): boolean {
  return Math.abs(a - b) < 1e-9
}

export function isExplicitParameter(doc: CadDocument, name: string): boolean {
  const value = doc.parameters?.[name]
  return value != null && Number.isFinite(value)
}

export function explicitParameterNames(doc: CadDocument): string[] {
  return Object.entries(doc.parameters ?? {})
    .filter(([, value]) => Number.isFinite(value))
    .map(([name]) => name)
}

export interface ParameterBatch {
  values?: Record<string, number>
  deletes?: string[]
}

/**
 * Apply every value edit, then drop deleted keys from `document.parameters`.
 * Feature literals are not rewritten on delete — only the map entry is removed.
 * Inferred-only names (no map entry) are ignored on delete; they are not in the map.
 */
export function applyParameterBatch(doc: CadDocument, batch: ParameterBatch): CadDocument {
  const deletes = new Set(batch.deletes ?? [])
  const values = batch.values ?? {}
  let next = doc
  for (const [name, value] of Object.entries(values)) {
    if (deletes.has(name)) continue
    next = setDocumentParameter(next, name, value)
  }
  if (deletes.size === 0) return next

  const parameters = { ...(next.parameters ?? {}) }
  let removed = false
  for (const name of deletes) {
    if (Object.prototype.hasOwnProperty.call(parameters, name)) {
      delete parameters[name]
      removed = true
    }
  }
  if (!removed) return next
  return {
    ...next,
    parameters: Object.keys(parameters).length > 0 ? parameters : undefined,
  }
}

export function parameterBatchHasWork(doc: CadDocument, batch: ParameterBatch): boolean {
  const deletes = batch.deletes ?? []
  if (deletes.some((name) => isExplicitParameter(doc, name))) return true
  const resolved = resolvedParameters(doc)
  for (const [name, value] of Object.entries(batch.values ?? {})) {
    if (deletes.includes(name)) continue
    const current = resolved[name]
    if (current == null || !sameParameterValue(current, value)) return true
  }
  return false
}

export function parameterBatchLabel(batch: ParameterBatch): string {
  const edits = Object.entries(batch.values ?? {})
  const deletes = batch.deletes ?? []
  const parts = [
    ...edits.map(([name, value]) => `${name} → ${value}`),
    ...deletes.map((name) => `delete ${name}`),
  ]
  if (parts.length === 0) return 'Parameters'
  if (parts.length === 1) return parts[0]!
  return `Parameters (${parts.length})`
}

/**
 * Turn panel drafts + pending deletes into one Calculate payload.
 * Enter/blur only update `drafts`; this runs when the user clicks Calculate.
 */
export function collectParameterBatch(opts: {
  committed: Record<string, number>
  explicitNames: readonly string[]
  drafts: Record<string, string>
  pendingDeletes: readonly string[]
}): { values: Record<string, number>; deletes: string[]; invalid: string[] } {
  const explicit = new Set(opts.explicitNames)
  const pending = new Set(opts.pendingDeletes)
  const deletes = [...pending].filter((name) => explicit.has(name))
  const values: Record<string, number> = {}
  const invalid: string[] = []

  for (const [name, current] of Object.entries(opts.committed)) {
    if (pending.has(name)) continue
    const raw = opts.drafts[name]
    if (raw == null) continue
    const parsed = parseParameterDraft(raw, name)
    if (parsed == null) {
      if (raw.trim() !== '' && raw.trim() !== String(current)) {
        invalid.push(name)
      }
      continue
    }
    if (!sameParameterValue(parsed, current)) {
      values[name] = parsed
    }
  }

  return { values, deletes, invalid }
}

/** Scale numbers that match common ratios of the old parameter (hex vertices, halves). */
function scaleLike(v: number, oldVal: number, newVal: number, key?: string): number | null {
  const tol = numberTol(oldVal)
  const axial = key === 'length' || key === 'depth' || key === 'height'
  // Axial lengths never use hex-vertex ratios (M8 shank 34.7 ≈ 40·√3/2).
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

function numberTol(oldVal: number): number {
  return Math.max(Math.abs(oldVal) * 0.002, 0.02)
}

/**
 * Kernel-aligned rewrite: overall length never ratio-scales hex `depth` or
 * independent head/dead literals. Length changes shank cylinder height and
 * thread extent only.
 */
function applyParameterDelta(
  doc: CadDocument,
  name: string,
  oldVal: number,
  newVal: number,
): CadDocument {
  const protectedVals = protectedParameterValues(doc.parameters, name)
  const axialOverall = isAxialOverallName(name)
  const replaced = { v: false }
  let next = rewriteNumbers(
    doc,
    oldVal,
    newVal,
    undefined,
    replaced,
    protectedVals,
    axialOverall,
  ) as CadDocument
  if (axialOverall && !replaced.v) {
    next = bumpShankAxialFields(next, newVal - oldVal) as CadDocument
  }
  return next
}

function protectedParameterValues(
  parameters: Record<string, number> | undefined,
  changing: string,
): number[] {
  if (!parameters) return []
  const out: number[] = []
  for (const [k, v] of Object.entries(parameters)) {
    if (k === changing) continue
    if (!isIndependentBoltDim(k)) continue
    if (!Number.isFinite(v) || Math.abs(v) < 0.05) continue
    out.push(v)
  }
  return out
}

function rewriteNumbers(
  node: unknown,
  oldVal: number,
  newVal: number,
  key: string | undefined,
  replaced: { v: boolean },
  protectedVals: number[],
  axialOverall: boolean,
): unknown {
  if (node && typeof node === 'object') {
    if (Array.isArray(node)) {
      return node.map((item) =>
        rewriteNumbers(item, oldVal, newVal, key, replaced, protectedVals, axialOverall),
      )
    }
    const rec = node as Record<string, unknown>
    const out: Record<string, unknown> = {}
    for (const [k, v] of Object.entries(rec)) {
      if (k === 'parameters') {
        out[k] = v
        continue
      }
      out[k] = rewriteNumbers(v, oldVal, newVal, k, replaced, protectedVals, axialOverall)
    }
    return out
  }
  if (typeof node === 'number') {
    if (key && SKIP_REWRITE_KEYS.has(key)) return node
    if (protectedVals.some((p) => Math.abs(node - p) <= Math.max(numberTol(p), 0.05))) {
      return node
    }
    // Hex / fuse extrude depth is the head, not the shank.
    if (axialOverall && key === 'depth') return node
    const nv = scaleLike(node, oldVal, newVal, key)
    if (nv != null) {
      if (Math.abs(node - oldVal) <= numberTol(oldVal)) replaced.v = true
      return nv
    }
  }
  return node
}

/** Grow/shrink shank cylinder `height` and thread `length` together. Never bump hex `depth`. */
function bumpShankAxialFields(node: unknown, delta: number): unknown {
  if (node && typeof node === 'object') {
    if (Array.isArray(node)) {
      return node.map((item) => bumpShankAxialFields(item, delta))
    }
    const rec = node as Record<string, unknown>
    const op = rec.op
    const shankOp = op === 'cylinder' || op === 'thread'
    const out: Record<string, unknown> = {}
    for (const [k, v] of Object.entries(rec)) {
      if (k === 'parameters') {
        out[k] = v
        continue
      }
      if (shankOp && (k === 'height' || k === 'length') && typeof v === 'number') {
        out[k] = Math.max(0.05, v + delta)
        continue
      }
      out[k] = bumpShankAxialFields(v, delta)
    }
    return out
  }
  return node
}

function isIndependentBoltDim(name: string): boolean {
  const n = name.toLowerCase()
  return (
    n === 'head_height' ||
    n === 'hex_height' ||
    n === 'head_width' ||
    n === 'hex_width' ||
    n === 'across_flats' ||
    n === 'dead_height' ||
    n === 'dead_length' ||
    n === 'unthreaded_length' ||
    n === 'unthreaded_height' ||
    n.includes('head_height') ||
    n.includes('dead_height') ||
    n.includes('dead_length') ||
    n.includes('unthreaded')
  )
}

function isHeadHeightName(name: string): boolean {
  const n = name.toLowerCase()
  return HEAD_HEIGHT_NAMES.includes(n) || n.includes('head_height')
}

function isDeadHeightName(name: string): boolean {
  const n = name.toLowerCase()
  return (
    DEAD_HEIGHT_NAMES.includes(n) ||
    n.includes('dead_height') ||
    n.includes('dead_length') ||
    n.includes('unthreaded')
  )
}

/** `dead_height` of 0 is a valid fastener (thread starts at the head). */
export function parameterAllowsZero(name: string): boolean {
  return isDeadHeightName(name)
}

/** Parse a typed parameter. Reject negatives; allow 0 only where physically valid. */
export function parseParameterDraft(raw: string, name: string): number | null {
  const value = Number.parseFloat(raw)
  if (!Number.isFinite(value)) return null
  if (parameterAllowsZero(name) ? value < 0 : value <= 0) return null
  return value
}

/** Slider span that stays usable when the current value is 0 or near-zero. */
export function sliderBounds(value: number, allowZero: boolean): { min: number; max: number } {
  const magnitude = Math.abs(value)
  const min = allowZero ? 0 : Math.max(0.1, magnitude * 0.1 || 0.1)
  const span = magnitude > 0.05 ? magnitude * 3 : 10
  const max = Math.max(min + 1, span, min * 2)
  return { min, max }
}

function isAxialOverallName(name: string): boolean {
  const n = name.toLowerCase()
  if (isIndependentBoltDim(n)) return false
  return (
    n === 'length' ||
    n === 'height' ||
    n === 'bolt_length' ||
    n === 'overall_length' ||
    n === 'total_length' ||
    (n.endsWith('_length') && !n.includes('head') && !n.includes('pitch') && !n.includes('dead'))
  )
}

function hasNamed(params: Record<string, number>, names: string[]): boolean {
  return names.some((n) => Number.isFinite(params[n]))
}

function featureHasHex(feat: Feature): boolean {
  if (feat.op === 'sketch' || feat.op === 'fuse' || feat.op === 'cut') {
    return 'hex' in feat.profile
  }
  return false
}

function hexBeforeThread(features: Feature[]): boolean {
  const hexI = features.findIndex(featureHasHex)
  const threadI = features.findIndex((f) => f.op === 'thread')
  if (hexI >= 0 && threadI >= 0) return hexI < threadI
  return hexI >= 0
}

/** Infer golden-M8 envelope dims from hex + thread feature literals. */
export function inferBoltParameters(doc: CadDocument): {
  bolt_length?: number
  head_height?: number
  dead_height?: number
} {
  for (const body of doc.bodies) {
    const dims = inferBoltDims(body.features ?? [])
    if (dims) return dims
  }
  return {}
}

function inferBoltDims(features: Feature[]): {
  bolt_length: number
  head_height: number
  dead_height: number
} | null {
  if (!features.some(featureHasHex) || !features.some((f) => f.op === 'thread')) {
    return null
  }
  const hexFirst = hexBeforeThread(features)
  let headHeight: number | undefined
  let pendingHex = false
  for (const feat of features) {
    if (featureHasHex(feat)) {
      if (feat.op === 'sketch') pendingHex = true
      if (feat.op === 'fuse' && typeof feat.depth === 'number') {
        headHeight = feat.depth
      }
    } else if (feat.op === 'extrude' && pendingHex && typeof feat.depth === 'number') {
      headHeight = feat.depth
      pendingHex = false
    } else if (feat.op !== 'sketch') {
      pendingHex = false
    }
  }
  if (headHeight == null || !(headHeight > 0)) return null

  const thread = features.find((f) => f.op === 'thread')
  const cyl = features.find((f) => f.op === 'cylinder')
  let boltLength: number | undefined
  let deadHeight = 0

  if (thread && thread.op === 'thread') {
    const tz = thread.at?.[2] ?? 0
    const tlen = thread.length
    if (typeof tlen === 'number' && tlen > 0) {
      if (hexFirst) {
        deadHeight = Math.max(0, tz - headHeight)
        boltLength = tz + tlen
      } else {
        boltLength = tlen + headHeight + deadHeight
      }
    }
  }
  if (boltLength == null && cyl && cyl.op === 'cylinder' && typeof cyl.height === 'number') {
    const cz = cyl.at?.[2] ?? 0
    boltLength = cz + cyl.height
  }
  if (boltLength == null || !(boltLength > 0)) return null
  return { bolt_length: boltLength, head_height: headHeight, dead_height: deadHeight }
}

/**
 * `head_height` patches hex depth and translates the shank/thread with the
 * head so the thread cannot start inside the new hex. Cylinder/thread axial
 * extents shrink or grow by the same delta so overall bolt length is kept.
 */
function applyHexHeadHeight(doc: CadDocument, oldVal: number, newVal: number): CadDocument {
  const delta = newVal - oldVal
  return {
    ...doc,
    bodies: doc.bodies.map((body) => ({
      ...body,
      features: shiftShankWithHead(patchHexDepth(body.features, newVal), delta),
    })),
  }
}

function shiftShankWithHead(features: Feature[], delta: number): Feature[] {
  if (!Number.isFinite(delta) || Math.abs(delta) < 1e-12) return features
  return features.map((feat) => {
    if (feat.op === 'cylinder') {
      const at = feat.at ?? [0, 0, 0]
      const next = {
        ...feat,
        at: [at[0], at[1], at[2] + delta] as [number, number, number],
      }
      if (typeof feat.height === 'number') {
        next.height = Math.max(0.05, feat.height - delta)
      }
      return next
    }
    if (feat.op === 'thread') {
      const at = feat.at ?? [0, 0, 0]
      const next: ThreadOp = {
        ...feat,
        at: [at[0], at[1], at[2] + delta],
      }
      if (typeof feat.length === 'number') {
        next.length = Math.max(0.05, feat.length - delta)
      }
      return next
    }
    return feat
  })
}

function patchHexDepth(features: Feature[], value: number): Feature[] {
  let pendingHex = false
  return features.map((feat) => {
    if (featureHasHex(feat)) {
      if (feat.op === 'sketch') {
        pendingHex = true
        return feat
      }
      if (feat.op === 'fuse') {
        pendingHex = false
        return { ...feat, depth: value }
      }
      pendingHex = false
      return feat
    }
    if (feat.op === 'extrude' && pendingHex) {
      pendingHex = false
      return { ...feat, depth: value }
    }
    if (feat.op !== 'sketch') pendingHex = false
    return feat
  })
}

/** `dead_height` moves thread start (and shortens/lengthens the bead) without touching hex. */
function applyDeadHeight(doc: CadDocument, oldVal: number, newVal: number): CadDocument {
  const delta = newVal - oldVal
  return {
    ...doc,
    bodies: doc.bodies.map((body) => ({
      ...body,
      features: body.features.map((feat) => {
        if (feat.op !== 'thread') return feat
        const at = feat.at ?? [0, 0, 0]
        const next: ThreadOp = {
          ...feat,
          at: [at[0], at[1], at[2] + delta],
        }
        if (typeof feat.length === 'number') {
          next.length = Math.max(0.05, feat.length - delta)
        }
        return next
      }),
    })),
  }
}

export function parameterEntries(doc: CadDocument): Array<[string, number]> {
  return Object.entries(resolvedParameters(doc)).sort(([a], [b]) => a.localeCompare(b))
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
