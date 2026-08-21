// ── JSON IR types (mirror of crates/kernel/src/ir.rs) ─────────────────────

export type Units = 'mm' | 'in'

export type SketchPlane = 'XY' | 'XZ' | 'YZ'

export type RectProfile   = { w: number; h: number; at?: [number, number]; centered?: boolean }
export type CircleProfile = { d: number; at?: [number, number] }
export type PolylineProfile = { points: [number, number][]; closed?: boolean }
export type ArcProfile = {
  center: [number, number]
  radius: number
  start_angle: number
  end_angle: number
}

export type EllipseProfile = {
  major: number
  minor: number
  at?: [number, number]
}

export type Profile =
  | { rect:     RectProfile }
  | { circle:   CircleProfile }
  | { polyline: PolylineProfile }
  | { arc:      ArcProfile }
  | { ellipse:  EllipseProfile }

export type EdgeSelection = 'all' | number[]

export type SketchOp = {
  op: 'sketch'
  id?: string
  plane?: SketchPlane
  profile: Profile
  origin?: [number, number]
}

export type ExtrudeOp = {
  op: 'extrude'
  id?: string
  depth: number
  symmetric?: boolean
}

export type RevolveOp = {
  op: 'revolve'
  id?: string
  angle?: number
  axis: 'X' | 'Y' | 'Z'
  origin?: [number, number, number]
}

export type CutOp = {
  op: 'cut'
  profile: Profile
  depth: number
  at?: [number, number, number]
  plane?: SketchPlane
  through?: boolean
}

export type FuseOp = {
  op: 'fuse'
  profile: Profile
  depth: number
  at?: [number, number, number]
  plane?: SketchPlane
}

export type HoleOp = {
  op: 'hole'
  diameter: number
  depth: number
  center: [number, number]
  plane?: SketchPlane
  through?: boolean
}

export type FilletOp = {
  op: 'fillet'
  radius: number
  edges?: EdgeSelection
}

export type ChamferOp = {
  op: 'chamfer'
  distance: number
  angle?: number
  edges?: EdgeSelection
}

export type TransformOp = {
  op: 'transform'
  translate?: [number, number, number]
  rotate?: { axis: [number, number, number]; angle: number; origin?: [number, number, number] }
  scale?: number
}

export type BoxOp = {
  op: 'box'
  size: [number, number, number]
  at?: [number, number, number]
  centered?: boolean
}

export type CylinderOp = {
  op: 'cylinder'
  diameter: number
  height: number
  at?: [number, number, number]
  axis?: 'X' | 'Y' | 'Z'
}

export type SphereOp = {
  op: 'sphere'
  diameter: number
  at?: [number, number, number]
}

export type ConeOp = {
  op: 'cone'
  d1: number
  d2: number
  height: number
  at?: [number, number, number]
}

export type TorusOp = {
  op: 'torus'
  major: number
  minor: number
  at?: [number, number, number]
}

export type LoftSection = {
  profile: Profile
  at?: [number, number, number]
}

export type LoftOp = {
  op: 'loft'
  sections: LoftSection[]
  ruled?: boolean
  apex?: [number, number, number]
}

export type MirrorOp = {
  op: 'mirror'
  plane: SketchPlane
  origin?: [number, number, number]
  fuse?: boolean
}

export type PatternOp = {
  op: 'pattern'
  kind: 'linear' | 'circular'
  count: number
  spacing?: number
  direction?: [number, number, number]
  axis?: 'X' | 'Y' | 'Z'
  angle?: number
  center?: [number, number, number]
}

export type ShellOp = {
  op: 'shell'
  thickness: number
  faces?: EdgeSelection
}

export type DraftExtrudeOp = {
  op: 'draft_extrude'
  depth: number
  angle: number
}

export type ThreadOp = {
  op: 'thread'
  kind: 'external' | 'internal' | 'die' | 'tap' | 'male' | 'bolt' | 'female' | 'nut'
  size?: string
  diameter?: number
  pitch?: number
  length?: number
  depth?: number
  at?: [number, number, number]
  axis?: 'X' | 'Y' | 'Z'
  center?: [number, number]
  plane?: SketchPlane
  through?: boolean
  hand?: 'right' | 'left'
}

export type SweepOp = {
  op: 'sweep'
  profile?: Profile
  path:
    | { helix: { pitch: number; height: number; radius: number; at?: [number, number, number]; axis?: 'X' | 'Y' | 'Z' } }
    | { polyline: { points: [number, number, number][] } }
}

export type HelixOp = {
  op: 'helix'
  pitch: number
  height: number
  radius: number
  section_diameter: number
  at?: [number, number, number]
  axis?: 'X' | 'Y' | 'Z'
}

export type OffsetOp = {
  op: 'offset'
  distance: number
}

export type ThickenOp = {
  op: 'thicken'
  thickness: number
}

export type CommonOp = {
  op: 'common'
  profile: Profile
  depth: number
  at?: [number, number, number]
  plane?: SketchPlane
}

export type EllipsoidOp = {
  op: 'ellipsoid'
  radii: [number, number, number]
  at?: [number, number, number]
}

export type DraftOp = {
  op: 'draft'
  angle: number
  pull?: 'X' | 'Y' | 'Z'
  faces?: EdgeSelection
}

export type Feature =
  | SketchOp | ExtrudeOp | RevolveOp
  | CutOp   | FuseOp    | HoleOp
  | FilletOp | ChamferOp | TransformOp
  | BoxOp | CylinderOp | SphereOp | ConeOp | TorusOp
  | LoftOp | MirrorOp | PatternOp | ShellOp | DraftExtrudeOp
  | ThreadOp | SweepOp | HelixOp | OffsetOp | ThickenOp
  | CommonOp | EllipsoidOp | DraftOp

export interface CadProgram {
  units: Units
  features: Feature[]
}

export interface BodyTransform {
  position: [number, number, number]
  rotation: [number, number, number]
}

export interface BodyReference {
  op: 'cut' | 'fuse'
  target: string
  consume?: boolean
}

export interface CadBody {
  bodyId: string
  name: string
  visible?: boolean
  suppressed?: boolean
  transform?: BodyTransform
  features: Feature[]
  references?: BodyReference[]
}

export interface CadDocument {
  documentId: string
  units: Units
  bodies: CadBody[]
}

export interface BodyInstance {
  bodyId: string
  name: string
  visible: boolean
  suppressed: boolean
  mesh: MeshData
  metrics: MetricsData
}

// ── API response types ────────────────────────────────────────────────────────

export interface MeshData {
  positions: number[]
  normals:   number[]
  indices:   number[]
}

export interface MetricsData {
  volume:       number
  /** [xmin, ymin, zmin, xmax, ymax, zmax] */
  bbox:         [number, number, number, number, number, number]
  surface_area: number
  is_solid:     boolean
}

export interface RunResponse {
  success:  boolean
  mesh?:    MeshData
  metrics?: MetricsData
  bodies?:  BodyInstance[]
  error?:   string
}

export type ExportFormat = 'step' | 'stl' | 'gltf' | 'glb' | 'obj' | 'brep'

// ── Chat ──────────────────────────────────────────────────────────────────────

export type ActivityKind = 'thinking' | 'writing' | 'calculating' | 'verifying' | 'rendering' | 'repair'

export interface ActivityStep {
  id:        string
  kind:      ActivityKind
  status:    'running' | 'done'
  startedAt: number
  ms?:       number
  detail?:   string
}

export interface ChatMessage {
  id:        string
  role:      'user' | 'assistant'
  content:   string
  timestamp: number
  steps?:    ActivityStep[]
}

export type ChatStreamEvent =
  | { type: 'thinking_start' }
  | { type: 'thinking_delta'; text: string }
  | { type: 'thinking_done'; ms: number }
  | { type: 'writing_start' }
  | { type: 'writing_done'; ms: number }
  | { type: 'repair'; attempt: number; error: string }
  | { type: 'calculating_start' }
  | { type: 'calculating_done'; ms: number }
  | { type: 'verifying_start' }
  | { type: 'verifying_done'; ms: number }
  | {
      type:      'result'
      success:   boolean
      message:   string
      program?:  CadDocument | CadProgram
      mesh?:     MeshData
      metrics?:  MetricsData
      bodies?:   BodyInstance[]
      error?:    string
      attempts:  number
    }
