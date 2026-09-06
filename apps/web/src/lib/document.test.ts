import assert from 'node:assert/strict'
import {
  applyParameterBatch,
  collectParameterBatch,
  explicitParameterNames,
  inferBoltParameters,
  isExplicitParameter,
  parameterAllowsZero,
  parameterBatchHasWork,
  parameterBatchLabel,
  parameterEntries,
  parseParameterDraft,
  parseSceneJson,
  prettyDocument,
  resolvedParameters,
  setDocumentParameter,
  sliderBounds,
} from './document.ts'
import type { CadDocument, CylinderOp, ExtrudeOp, Feature, ThreadOp } from '../types/cad.ts'

/** Golden M8×40 IR with numeric literals and no parameters map. */
function goldenM8NoParams(overrides?: {
  extrudeDepth?: number
  cylHeight?: number
  cylZ?: number
  threadLength?: number
  threadZ?: number
}): string {
  const extrudeDepth = overrides?.extrudeDepth ?? 5.3
  const cylHeight = overrides?.cylHeight ?? 35.7
  const cylZ = overrides?.cylZ ?? 4.3
  const threadLength = overrides?.threadLength ?? 34.7
  const threadZ = overrides?.threadZ ?? 5.3
  return JSON.stringify({
    documentId: 'm8_bolt_40mm',
    units: 'mm',
    bodies: [
      {
        bodyId: 'body_m8_bolt',
        name: 'M8 Bolt',
        visible: true,
        suppressed: false,
        transform: { position: [0, 0, 0], rotation: [0, 0, 0] },
        features: [
          {
            id: 'sketch',
            op: 'sketch',
            origin: [0, 0],
            plane: 'XY',
            profile: { hex: { across_flats: 13, at: [0, 0] } },
          },
          { depth: extrudeDepth, id: 'body', op: 'extrude', symmetric: false },
          { at: [0, 0, cylZ], axis: 'Z', diameter: 8, height: cylHeight, op: 'cylinder' },
          {
            at: [0, 0, threadZ],
            axis: 'Z',
            center: [0, 0],
            hand: 'right',
            kind: 'external',
            length: threadLength,
            op: 'thread',
            plane: 'XY',
            size: 'M8',
            through: false,
          },
        ],
        references: [],
      },
    ],
  })
}

function feat<T extends Feature['op']>(doc: CadDocument, op: T): Extract<Feature, { op: T }> {
  const f = doc.bodies[0].features.find((x) => x.op === op)
  assert.ok(f, `missing ${op}`)
  return f as Extract<Feature, { op: T }>
}

function almost(a: number, b: number, eps = 1e-9) {
  assert.ok(Math.abs(a - b) <= eps, `expected ${a} ≈ ${b}`)
}

// 1. Panel entries without a parameters map
{
  const doc = parseSceneJson(goldenM8NoParams())
  const inferred = inferBoltParameters(doc)
  almost(inferred.bolt_length ?? NaN, 40)
  almost(inferred.head_height ?? NaN, 5.3)
  almost(inferred.dead_height ?? NaN, 0)

  const names = parameterEntries(doc).map(([n]) => n)
  assert.ok(names.includes('bolt_length'), names.join(','))
  assert.ok(names.includes('head_height'), names.join(','))
  assert.ok(names.includes('dead_height'), names.join(','))
  const params = resolvedParameters(doc)
  almost(params.bolt_length, 40)
  almost(params.head_height, 5.3)
  almost(params.dead_height, 0)
}

// Dead under head is inferred from thread start
{
  const doc = parseSceneJson(goldenM8NoParams({ threadZ: 7.3, threadLength: 32.7 }))
  const params = resolvedParameters(doc)
  almost(params.bolt_length, 40)
  almost(params.head_height, 5.3)
  almost(params.dead_height, 2)
}

// Non-bolt documents stay empty
{
  const doc = parseSceneJson(
    JSON.stringify({
      documentId: 'box',
      units: 'mm',
      bodies: [{ bodyId: 'b', name: 'Box', features: [{ op: 'box', size: [10, 10, 10] }] }],
    }),
  )
  assert.deepEqual(parameterEntries(doc), [])
}

// 2. bolt_length commit updates shank/thread only
{
  const doc = parseSceneJson(goldenM8NoParams())
  const next = setDocumentParameter(doc, 'bolt_length', 50)
  const hex = feat(next, 'extrude') as ExtrudeOp
  const cyl = feat(next, 'cylinder') as CylinderOp
  const thread = feat(next, 'thread') as ThreadOp
  almost(hex.depth, 5.3)
  almost(cyl.height, 45.7)
  almost(cyl.at![2], 4.3)
  almost(thread.length!, 44.7)
  almost(thread.at![2], 5.3)
  almost(next.parameters!.bolt_length, 50)
  almost(next.parameters!.head_height, 5.3)
  almost(next.parameters!.dead_height, 0)
}

// Length does not ratio-scale hex depth even when depth == L or L/2
{
  const doc = parseSceneJson(goldenM8NoParams({ extrudeDepth: 40 }))
  const next = setDocumentParameter(doc, 'bolt_length', 64)
  const hex = feat(next, 'extrude') as ExtrudeOp
  assert.ok(Math.abs(hex.depth - 64) > 1, `hex was ratio-scaled to ${hex.depth}`)
  almost(hex.depth, 40)
}

{
  const doc = parseSceneJson(goldenM8NoParams({ extrudeDepth: 20 }))
  const next = setDocumentParameter(doc, 'bolt_length', 50)
  almost((feat(next, 'extrude') as ExtrudeOp).depth, 20)
}

// 3. head_height commit moves hex and shank/thread together
{
  const doc = parseSceneJson(goldenM8NoParams())
  const next = setDocumentParameter(doc, 'head_height', 8)
  const hex = feat(next, 'extrude') as ExtrudeOp
  const cyl = feat(next, 'cylinder') as CylinderOp
  const thread = feat(next, 'thread') as ThreadOp
  const delta = 8 - 5.3
  almost(hex.depth, 8)
  almost(cyl.at![2], 4.3 + delta)
  almost(cyl.height, 35.7 - delta)
  almost(thread.at![2], 5.3 + delta)
  almost(thread.length!, 34.7 - delta)
  almost(thread.at![2] + thread.length!, 40)
  almost(next.parameters!.head_height, 8)
  almost(next.parameters!.bolt_length, 40)
  almost(next.parameters!.dead_height, 0)
  const inferred = inferBoltParameters(next)
  almost(inferred.bolt_length ?? NaN, 40)
  almost(inferred.head_height ?? NaN, 8)
  almost(inferred.dead_height ?? NaN, 0)
}

// head_height shrink moves shank back and preserves overall length
{
  const doc = parseSceneJson(goldenM8NoParams())
  const next = setDocumentParameter(doc, 'head_height', 4)
  const delta = 4 - 5.3
  const cyl = feat(next, 'cylinder') as CylinderOp
  const thread = feat(next, 'thread') as ThreadOp
  almost((feat(next, 'extrude') as ExtrudeOp).depth, 4)
  almost(cyl.at![2], 4.3 + delta)
  almost(cyl.height, 35.7 - delta)
  almost(thread.at![2], 5.3 + delta)
  almost(thread.length!, 34.7 - delta)
  almost(inferBoltParameters(next).bolt_length ?? NaN, 40)
}

// head_height keeps an existing dead under the head
{
  const doc = parseSceneJson(goldenM8NoParams({ threadZ: 7.3, threadLength: 32.7 }))
  const next = setDocumentParameter(doc, 'head_height', 8)
  const thread = feat(next, 'thread') as ThreadOp
  const delta = 8 - 5.3
  almost(thread.at![2], 7.3 + delta)
  almost(thread.length!, 32.7 - delta)
  almost(inferBoltParameters(next).dead_height ?? NaN, 2)
  almost(inferBoltParameters(next).bolt_length ?? NaN, 40)
}

// dead_height 0 is valid and can be committed
{
  const doc = parseSceneJson(goldenM8NoParams({ threadZ: 7.3, threadLength: 32.7 }))
  almost(resolvedParameters(doc).dead_height, 2)
  const next = setDocumentParameter(doc, 'dead_height', 0)
  const thread = feat(next, 'thread') as ThreadOp
  almost(thread.at![2], 5.3)
  almost(thread.length!, 34.7)
  almost(next.parameters!.dead_height, 0)
  almost(inferBoltParameters(next).dead_height ?? NaN, 0)
}

{
  assert.equal(parameterAllowsZero('dead_height'), true)
  assert.equal(parameterAllowsZero('unthreaded_length'), true)
  assert.equal(parameterAllowsZero('bolt_length'), false)
  assert.equal(parameterAllowsZero('head_height'), false)
  assert.equal(parseParameterDraft('0', 'dead_height'), 0)
  assert.equal(parseParameterDraft('0', 'bolt_length'), null)
  assert.equal(parseParameterDraft('-1', 'dead_height'), null)
  assert.equal(parseParameterDraft('5.3', 'head_height'), 5.3)
  const zeroSlider = sliderBounds(0, true)
  assert.equal(zeroSlider.min, 0)
  assert.ok(zeroSlider.max > zeroSlider.min, 'zero slider must not collapse')
  const tiny = sliderBounds(0.01, true)
  assert.equal(tiny.min, 0)
  assert.ok(tiny.max > 1)
}

// 4. Unchanged value does not rewrite feature literals
{
  const doc = parseSceneJson(goldenM8NoParams())
  const next = setDocumentParameter(doc, 'bolt_length', 40)
  const hex = feat(next, 'extrude') as ExtrudeOp
  const cyl = feat(next, 'cylinder') as CylinderOp
  const thread = feat(next, 'thread') as ThreadOp
  almost(hex.depth, 5.3)
  almost(cyl.height, 35.7)
  almost(thread.length!, 34.7)
}

// Commit-guard used by the panel / store: same value is a no-op
{
  const doc = parseSceneJson(goldenM8NoParams())
  const current = resolvedParameters(doc).bolt_length
  assert.equal(current, 40)
  const shouldRebuild = Math.abs(40 - current) >= 1e-9
  assert.equal(shouldRebuild, false)
}

// 5. Batch calculate: several dirty drafts become one document, one rebuild payload
{
  const doc = parseSceneJson(goldenM8NoParams())
  const next = applyParameterBatch(doc, {
    values: { bolt_length: 50, head_height: 7, dead_height: 2 },
  })
  const hex = feat(next, 'extrude') as ExtrudeOp
  const cyl = feat(next, 'cylinder') as CylinderOp
  const thread = feat(next, 'thread') as ThreadOp
  almost(hex.depth, 7)
  almost(next.parameters!.bolt_length, 50)
  almost(next.parameters!.head_height, 7)
  almost(next.parameters!.dead_height, 2)
  almost(cyl.height + (cyl.at![2] ?? 0), 50, 0.05)
  almost(thread.at![2]! + thread.length!, 50, 0.05)
  assert.equal(parameterBatchHasWork(doc, { values: { bolt_length: 40 } }), false)
  assert.equal(
    parameterBatchHasWork(doc, { values: { bolt_length: 50, head_height: 7 } }),
    true,
  )
  assert.equal(parameterBatchLabel({ values: { bolt_length: 50 } }), 'bolt_length → 50')
  assert.equal(
    parameterBatchLabel({
      values: { bolt_length: 50, head_height: 7 },
      deletes: ['head_width'],
    }),
    'Parameters (3)',
  )
  // Original document is not mutated (panel drafts stay local until Calculate).
  assert.equal(doc.parameters, undefined)
}

{
  const committed = resolvedParameters(parseSceneJson(goldenM8NoParams()))
  const collected = collectParameterBatch({
    committed,
    explicitNames: [],
    drafts: { bolt_length: '50', head_height: '7', dead_height: '0' },
    pendingDeletes: [],
  })
  assert.deepEqual(collected.invalid, [])
  almost(collected.values.bolt_length, 50)
  almost(collected.values.head_height, 7)
  assert.equal(collected.values.dead_height, undefined, 'unchanged dead_height is not dirty')
  assert.deepEqual(collected.deletes, [])
}

{
  const collected = collectParameterBatch({
    committed: { bolt_length: 50, head_width: 15 },
    explicitNames: ['bolt_length', 'head_width'],
    drafts: { bolt_length: 'nope', head_width: '16' },
    pendingDeletes: [],
  })
  assert.deepEqual(collected.invalid, ['bolt_length'])
  almost(collected.values.head_width, 16)
}

// 6. Delete removes explicit map entries; inferred-only delete is a no-op
{
  const doc = setDocumentParameter(parseSceneJson(goldenM8NoParams()), 'bolt_length', 50)
  assert.equal(isExplicitParameter(doc, 'bolt_length'), true)
  assert.ok(explicitParameterNames(doc).includes('bolt_length'))

  const deleted = applyParameterBatch(doc, { deletes: ['bolt_length'] })
  assert.equal(isExplicitParameter(deleted, 'bolt_length'), false)
  assert.equal(deleted.parameters?.bolt_length, undefined)
  // Feature literals stay at the last committed values (no reverse rewrite).
  almost((feat(deleted, 'cylinder') as CylinderOp).height, (feat(doc, 'cylinder') as CylinderOp).height)
  // Envelope dim is still inferable, so the panel may re-show it as inferred-only.
  almost(resolvedParameters(deleted).bolt_length, 50)
  assert.equal(parameterBatchHasWork(doc, { deletes: ['bolt_length'] }), true)
}

{
  const inferredOnly = parseSceneJson(goldenM8NoParams())
  assert.equal(isExplicitParameter(inferredOnly, 'bolt_length'), false)
  const after = applyParameterBatch(inferredOnly, { deletes: ['bolt_length'] })
  assert.equal(prettyDocument(after), prettyDocument(inferredOnly))
  assert.equal(parameterBatchHasWork(inferredOnly, { deletes: ['bolt_length'] }), false)
  const collected = collectParameterBatch({
    committed: resolvedParameters(inferredOnly),
    explicitNames: explicitParameterNames(inferredOnly),
    drafts: {},
    pendingDeletes: ['bolt_length'],
  })
  assert.deepEqual(collected.deletes, [], 'inferred-only pending delete is not in the map')
}

// 7. Calculate batches value edits and deletes in one document
{
  const doc = parseSceneJson(
    JSON.stringify({
      ...JSON.parse(goldenM8NoParams()),
      parameters: {
        bolt_length: 50,
        dead_height: 20,
        head_height: 7,
        head_width: 15,
      },
    }),
  )
  const next = applyParameterBatch(doc, {
    values: { bolt_length: 55, dead_height: 18 },
    deletes: ['head_width'],
  })
  almost(next.parameters!.bolt_length, 55)
  almost(next.parameters!.dead_height, 18)
  almost(next.parameters!.head_height, 7)
  assert.equal(next.parameters!.head_width, undefined)
  assert.ok(!explicitParameterNames(next).includes('head_width'))
  // Delete-of-dirty-value is skipped (the name is not written, then dropped).
  const skipDeletedEdit = applyParameterBatch(doc, {
    values: { head_width: 20 },
    deletes: ['head_width'],
  })
  assert.equal(skipDeletedEdit.parameters!.head_width, undefined)
}

{
  const collected = collectParameterBatch({
    committed: {
      bolt_length: 50,
      dead_height: 20,
      head_height: 7,
      head_width: 15,
    },
    explicitNames: ['bolt_length', 'dead_height', 'head_height', 'head_width'],
    drafts: { bolt_length: '55', dead_height: '18', head_width: '99' },
    pendingDeletes: ['head_width'],
  })
  almost(collected.values.bolt_length, 55)
  almost(collected.values.dead_height, 18)
  assert.equal(collected.values.head_width, undefined)
  assert.deepEqual(collected.deletes, ['head_width'])
  assert.deepEqual(collected.invalid, [])
}

console.log('document.test.ts: all assertions passed')
