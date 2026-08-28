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
