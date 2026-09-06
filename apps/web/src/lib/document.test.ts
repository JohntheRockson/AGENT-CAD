import assert from 'node:assert/strict'
import {
  applyParameterBatch,
  bodyDisplayName,
  chatInputTrustNote,
  chatSendConfirmMessage,
  collectParameterBatch,
  committedParametersSignature,
  countUncommittedParameters,
  deleteBodyTimelineLabel,
  documentForAgent,
  documentsAlign,
  editorTrustKind,
  explicitParameterNames,
  hideShowTimelineLabel,
  inferBoltParameters,
  irAfterBodyRemoval,
  isExplicitParameter,
  metricsFromBodies,
  parameterAllowsZero,
  parameterBatchHasWork,
  parameterBatchLabel,
  parameterEntries,
  parametersLastGoodNote,
  parseDocumentOrNull,
  parseParameterDraft,
  parseSceneJson,
  outlinerLastGoodNote,
  planDeleteBody,
  planRenameBody,
  planSetBodyVisible,
  prettyDocument,
  reconcileParameterDrafts,
  removeBodyFromDocument,
  renameBodyInDocument,
  renameBodyTimelineLabel,
  resolvedParameters,
  retainBodySelection,
  setBodyVisibleInDocument,
  setDocumentParameter,
  shouldConfirmChatSend,
  sliderBounds,
  targetBodyIdForDocument,
  toolbarRewriteConfirmMessage,
  uncommittedParameterChatWarning,
  uncommittedParameterExportNote,
  workingDocument,
} from './document.ts'
import type { BodyInstance, CadDocument, CylinderOp, ExtrudeOp, Feature, MetricsData, ThreadOp } from '../types/cad.ts'

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

// 8. Drafts survive cosmetic IR updates; prune after Calculate / lost names
{
  const committed = { bolt_length: 40, head_height: 5.3, dead_height: 0 }
  const explicit = ['bolt_length', 'head_height', 'dead_height']
  const kept = reconcileParameterDrafts({
    committed,
    explicitNames: explicit,
    drafts: { bolt_length: '50', head_height: '5.3', dead_height: 'nope' },
    pendingDeletes: ['head_height'],
  })
  // Cosmetic IR (rename / Run pretty-print): same committed values keep dirty + invalid.
  almost(Number(kept.drafts.bolt_length), 50)
  assert.equal(kept.drafts.head_height, undefined, 'draft matching committed is pruned')
  assert.equal(kept.drafts.dead_height, 'nope')
  assert.deepEqual(kept.pendingDeletes, ['head_height'])
  assert.equal(
    committedParametersSignature(committed),
    committedParametersSignature({ dead_height: 0, bolt_length: 40, head_height: 5.3 }),
  )
}

{
  // After Calculate commits 50, the matching draft is no longer dirty.
  const afterCalc = reconcileParameterDrafts({
    committed: { bolt_length: 50, head_height: 5.3, dead_height: 0 },
    explicitNames: ['bolt_length', 'head_height', 'dead_height'],
    drafts: { bolt_length: '50' },
    pendingDeletes: [],
  })
  assert.deepEqual(afterCalc.drafts, {})
}

{
  // Deleted explicit name that reappears as inferred-only must not stay pending-delete.
  const afterDelete = reconcileParameterDrafts({
    committed: { bolt_length: 50, head_height: 5.3, dead_height: 0 },
    explicitNames: ['head_height', 'dead_height'],
    drafts: { bolt_length: '55', gone: '1' },
    pendingDeletes: ['bolt_length', 'gone'],
  })
  assert.equal(afterDelete.drafts.gone, undefined)
  almost(Number(afterDelete.drafts.bolt_length), 55)
  assert.deepEqual(afterDelete.pendingDeletes, [])
}

{
  assert.ok(
    uncommittedParameterChatWarning(2).includes('last calculated model'),
  )
  assert.ok(uncommittedParameterChatWarning(1).includes('1 uncommitted'))
  assert.ok(uncommittedParameterExportNote(2).includes('export is the last calculated'))
  const rewrite = toolbarRewriteConfirmMessage(0)
  assert.ok(rewrite.includes('loaded golden'))
  assert.ok(!rewrite.includes('uncommitted'))
  const rewriteDirty = toolbarRewriteConfirmMessage(1)
  assert.ok(rewriteDirty.includes('1 uncommitted parameter change'))
  assert.ok(rewriteDirty.includes('will not be sent to the agent'))
  assert.equal(
    countUncommittedParameters({
      values: { bolt_length: 50 },
      deletes: ['head_width'],
      invalid: ['dead_height'],
    }),
    3,
  )
}

// 9. Chat / Calculate use last-good when editor JSON is dirty or invalid
{
  const lastGood = prettyDocument(parseSceneJson(goldenM8NoParams()))
  const dirty = prettyDocument(parseSceneJson(goldenM8NoParams({ cylHeight: 99 })))
  const invalid = '{ "bodies": [ }'
  const spaced = `  ${lastGood}  \n`

  assert.equal(editorTrustKind(lastGood, lastGood), 'aligned')
  assert.equal(editorTrustKind(spaced, lastGood), 'aligned', 'whitespace-only stays aligned')
  assert.equal(editorTrustKind(dirty, lastGood), 'dirty')
  assert.equal(editorTrustKind(invalid, lastGood), 'invalid')
  assert.equal(editorTrustKind('', lastGood), 'dirty', 'cleared editor is dirty vs last-good')
  assert.equal(editorTrustKind(dirty, ''), 'dirty')
  assert.equal(editorTrustKind('', ''), 'empty')
  assert.equal(editorTrustKind('{', ''), 'empty')

  assert.ok(documentsAlign(parseSceneJson(lastGood), parseSceneJson(spaced)))
  assert.equal(parseDocumentOrNull(invalid), null)

  // Agent always sees last-good when it exists (viewport / export truth).
  assert.ok(documentsAlign(documentForAgent(dirty, lastGood)!, parseSceneJson(lastGood)))
  assert.ok(documentsAlign(documentForAgent(invalid, lastGood)!, parseSceneJson(lastGood)))
  assert.ok(documentsAlign(documentForAgent('', lastGood)!, parseSceneJson(lastGood)))
  assert.ok(documentsAlign(documentForAgent(lastGood, lastGood)!, parseSceneJson(lastGood)))
  assert.equal(documentForAgent(invalid, ''), null)
  assert.ok(documentsAlign(documentForAgent(dirty, '')!, parseSceneJson(dirty)))

  // Calculate prefers parseable editor JSON; typo falls back to last-good.
  assert.ok(documentsAlign(workingDocument(dirty, lastGood)!, parseSceneJson(dirty)))
  assert.ok(documentsAlign(workingDocument(invalid, lastGood)!, parseSceneJson(lastGood)))
  assert.ok(documentsAlign(workingDocument('', lastGood)!, parseSceneJson(lastGood)))

  assert.equal(
    shouldConfirmChatSend({ editorKind: 'aligned', hasLastGood: true, dirtyParamCount: 0 }),
    false,
  )
  assert.equal(
    shouldConfirmChatSend({ editorKind: 'dirty', hasLastGood: true, dirtyParamCount: 0 }),
    true,
  )
  assert.equal(
    shouldConfirmChatSend({ editorKind: 'invalid', hasLastGood: true, dirtyParamCount: 0 }),
    true,
  )
  assert.equal(
    shouldConfirmChatSend({ editorKind: 'dirty', hasLastGood: false, dirtyParamCount: 0 }),
    false,
    'never-run paste does not confirm',
  )
  assert.equal(
    shouldConfirmChatSend({ editorKind: 'aligned', hasLastGood: true, dirtyParamCount: 1 }),
    true,
  )

  const dirtyConfirm = chatSendConfirmMessage({
    editorKind: 'dirty',
    hasLastGood: true,
    dirtyParamCount: 0,
  })
  assert.ok(dirtyConfirm.includes('unrun JSON editor edits'))
  assert.ok(dirtyConfirm.includes('last calculated model'))
  const bothConfirm = chatSendConfirmMessage({
    editorKind: 'invalid',
    hasLastGood: true,
    dirtyParamCount: 2,
  })
  assert.ok(bothConfirm.includes('2 uncommitted parameter changes'))
  assert.ok(bothConfirm.includes('invalid JSON'))

  assert.ok(
    chatInputTrustNote({
      editorKind: 'aligned',
      hasLastGood: true,
      dirtyParamCount: 1,
    })?.includes('last calculated model until Calculate'),
  )
  assert.ok(
    chatInputTrustNote({
      editorKind: 'dirty',
      hasLastGood: true,
      dirtyParamCount: 0,
    })?.includes('until Run'),
  )
  assert.equal(
    chatInputTrustNote({
      editorKind: 'dirty',
      hasLastGood: false,
      dirtyParamCount: 0,
    }),
    null,
  )
  assert.ok(
    parametersLastGoodNote({ editorKind: 'invalid', showingLastGood: true })?.includes('JSON is invalid'),
  )
  assert.equal(parametersLastGoodNote({ editorKind: 'dirty', showingLastGood: false }), null)
  assert.ok(
    outlinerLastGoodNote({ editorKind: 'invalid', showingLastGood: true })?.includes('replace the broken editor'),
  )
  assert.ok(
    outlinerLastGoodNote({ editorKind: 'dirty', showingLastGood: true })?.includes('JSON editor is empty'),
  )
  assert.equal(outlinerLastGoodNote({ editorKind: 'invalid', showingLastGood: false }), null)

  const rewriteEditor = toolbarRewriteConfirmMessage(0, 'dirty')
  assert.ok(rewriteEditor.includes('Unrun JSON editor edits will not be sent'))
  assert.ok(toolbarRewriteConfirmMessage(1, 'invalid').includes('Invalid editor JSON'))
}

// 10. Delete-body: History snapshot + metrics; dirty editor does not strip the solid
{
  const twoBody = parseSceneJson(JSON.stringify({
    documentId: 'two',
    units: 'mm',
    bodies: [
      {
        bodyId: 'body_a',
        name: 'Bolt',
        visible: true,
        features: [{ op: 'box', size: [10, 10, 10] }],
      },
      {
        bodyId: 'body_b',
        name: 'Nut',
        visible: true,
        features: [{ op: 'cylinder', diameter: 8, height: 6 }],
      },
    ],
  }))
  const aligned = prettyDocument(twoBody)
  const removed = removeBodyFromDocument(twoBody, 'body_a')
  assert.ok(removed)
  assert.equal(removed.bodies.length, 1)
  assert.equal(removed.bodies[0].bodyId, 'body_b')
  assert.equal(twoBody.bodies.length, 2, 'source document is not mutated')
  assert.equal(removeBodyFromDocument(twoBody, 'missing'), null)
  const lastRemoved = removeBodyFromDocument(removed, 'body_b')
  assert.ok(lastRemoved)
  assert.equal(irAfterBodyRemoval(lastRemoved), '')
  assert.equal(deleteBodyTimelineLabel('Bolt'), 'Delete Bolt')
  assert.equal(deleteBodyTimelineLabel('  '), 'Delete body')

  const scene = planDeleteBody({
    irCode: aligned,
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
  })
  assert.ok(scene && scene.kind === 'scene')
  assert.equal(scene.label, 'Delete Bolt')
  assert.ok(documentsAlign(parseSceneJson(scene.nextIrCode), removed))
  // Chat / export last-good after a scene delete is the remaining solid.
  assert.ok(
    documentsAlign(documentForAgent(scene.nextIrCode, scene.nextIrCode)!, removed),
  )

  const last = planDeleteBody({
    irCode: scene.nextIrCode,
    lastGoodIrCode: scene.nextIrCode,
    bodyId: 'body_b',
  })
  assert.ok(last && last.kind === 'scene')
  assert.equal(last.nextIrCode, '')

  const dirtyIr = prettyDocument({
    ...twoBody,
    bodies: twoBody.bodies.map((b) =>
      b.bodyId === 'body_b' ? { ...b, name: 'Nut draft' } : b,
    ),
  })
  assert.notEqual(dirtyIr, aligned)
  const editorOnly = planDeleteBody({
    irCode: dirtyIr,
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
  })
  assert.ok(editorOnly && editorOnly.kind === 'editor-only')
  assert.equal(parseDocumentOrNull(editorOnly.nextIrCode)?.bodies.length, 1)
  assert.equal(parseDocumentOrNull(editorOnly.nextIrCode)?.bodies[0].bodyId, 'body_b')
  // Viewport / chat / export stay on last-good (still has both bodies).
  assert.ok(documentsAlign(documentForAgent(editorOnly.nextIrCode, aligned)!, twoBody))
  assert.equal(
    planDeleteBody({ irCode: aligned, lastGoodIrCode: aligned, bodyId: 'nope' }),
    null,
  )
}

{
  const mesh = { positions: [0], normals: [0], indices: [0] }
  const metric = (partial: Partial<MetricsData> & { volume: number; bbox: MetricsData['bbox'] }): MetricsData => ({
    surface_area: partial.surface_area ?? 1,
    is_solid: partial.is_solid ?? true,
    units: partial.units ?? 'mm',
    volume: partial.volume,
    bbox: partial.bbox,
  })
  const inst = (id: string, m: MetricsData): BodyInstance => ({
    bodyId: id,
    name: id,
    visible: true,
    suppressed: false,
    mesh,
    metrics: m,
  })

  assert.equal(metricsFromBodies([]), null)
  const one = metricsFromBodies([
    inst('a', metric({ volume: 10, surface_area: 4, bbox: [0, 0, 0, 2, 2, 2] })),
  ])
  almost(one!.volume, 10)
  assert.deepEqual(one!.bbox, [0, 0, 0, 2, 2, 2])

  const combined = metricsFromBodies([
    inst('a', metric({ volume: 10, surface_area: 4, bbox: [0, 0, 0, 2, 2, 2] })),
    inst('b', metric({ volume: 3, surface_area: 5, is_solid: false, bbox: [-1, 1, 0, 1, 4, 3] })),
  ])
  almost(combined!.volume, 13)
  almost(combined!.surface_area, 9)
  assert.equal(combined!.is_solid, false)
  assert.deepEqual(combined!.bbox, [-1, 0, 0, 2, 4, 3])
}

// 11. Hide / rename: History when aligned; dirty editor does not hide the solid
{
  const twoBody = parseSceneJson(JSON.stringify({
    documentId: 'two',
    units: 'mm',
    bodies: [
      {
        bodyId: 'body_a',
        name: 'Bolt',
        visible: true,
        features: [{ op: 'box', size: [10, 10, 10] }],
      },
      {
        bodyId: 'body_b',
        name: 'Nut',
        visible: true,
        features: [{ op: 'cylinder', diameter: 8, height: 6 }],
      },
    ],
  }))
  const aligned = prettyDocument(twoBody)

  assert.equal(bodyDisplayName({ name: '  Bolt  ', bodyId: 'body_a' }), 'Bolt')
  assert.equal(hideShowTimelineLabel('Bolt', false), 'Hide Bolt')
  assert.equal(hideShowTimelineLabel('Bolt', true), 'Show Bolt')
  assert.equal(renameBodyTimelineLabel('Bolt', 'Hex bolt'), 'Rename Bolt → Hex bolt')
  assert.equal(renameBodyTimelineLabel('  ', 'x'), 'Rename body → x')

  const hiddenDoc = setBodyVisibleInDocument(twoBody, 'body_a', false)
  assert.ok(hiddenDoc)
  assert.equal(hiddenDoc.bodies.find((b) => b.bodyId === 'body_a')?.visible, false)
  assert.equal(twoBody.bodies.find((b) => b.bodyId === 'body_a')?.visible, true, 'source not mutated')
  assert.equal(setBodyVisibleInDocument(twoBody, 'body_a', true), null, 'already visible')
  assert.equal(setBodyVisibleInDocument(twoBody, 'missing', false), null)

  const renamedDoc = renameBodyInDocument(twoBody, 'body_a', '  Hex bolt  ')
  assert.ok(renamedDoc)
  assert.equal(renamedDoc.bodies.find((b) => b.bodyId === 'body_a')?.name, 'Hex bolt')
  assert.equal(twoBody.bodies.find((b) => b.bodyId === 'body_a')?.name, 'Bolt')
  assert.equal(renameBodyInDocument(twoBody, 'body_a', 'Bolt'), null)
  assert.equal(renameBodyInDocument(twoBody, 'body_a', '   '), null)
  assert.equal(renameBodyInDocument(twoBody, 'missing', 'X'), null)

  const hideScene = planSetBodyVisible({
    irCode: aligned,
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
    visible: false,
  })
  assert.ok(hideScene && hideScene.kind === 'scene')
  assert.equal(hideScene.label, 'Hide Bolt')
  assert.ok(documentsAlign(parseSceneJson(hideScene.nextIrCode), hiddenDoc))
  // Chat / export last-good after an aligned hide is the hidden solid.
  assert.ok(
    documentsAlign(documentForAgent(hideScene.nextIrCode, hideScene.nextIrCode)!, hiddenDoc),
  )

  const showScene = planSetBodyVisible({
    irCode: hideScene.nextIrCode,
    lastGoodIrCode: hideScene.nextIrCode,
    bodyId: 'body_a',
    visible: true,
  })
  assert.ok(showScene && showScene.kind === 'scene')
  assert.equal(showScene.label, 'Show Bolt')

  const renameScene = planRenameBody({
    irCode: aligned,
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
    name: 'Hex bolt',
  })
  assert.ok(renameScene && renameScene.kind === 'scene')
  assert.equal(renameScene.label, 'Rename Bolt → Hex bolt')
  assert.ok(documentsAlign(parseSceneJson(renameScene.nextIrCode), renamedDoc))

  const dirtyIr = prettyDocument({
    ...twoBody,
    bodies: twoBody.bodies.map((b) =>
      b.bodyId === 'body_b' ? { ...b, name: 'Nut draft' } : b,
    ),
  })
  assert.notEqual(dirtyIr, aligned)

  const hideDraft = planSetBodyVisible({
    irCode: dirtyIr,
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
    visible: false,
  })
  assert.ok(hideDraft && hideDraft.kind === 'editor-only')
  assert.equal(parseDocumentOrNull(hideDraft.nextIrCode)?.bodies.find((b) => b.bodyId === 'body_a')?.visible, false)
  // Viewport / chat / export stay on last-good (Bolt still visible).
  assert.ok(documentsAlign(documentForAgent(hideDraft.nextIrCode, aligned)!, twoBody))
  assert.equal(
    parseDocumentOrNull(aligned)?.bodies.find((b) => b.bodyId === 'body_a')?.visible !== false,
    true,
  )

  const renameDraft = planRenameBody({
    irCode: dirtyIr,
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
    name: 'Hex bolt',
  })
  assert.ok(renameDraft && renameDraft.kind === 'editor-only')
  assert.equal(parseDocumentOrNull(renameDraft.nextIrCode)?.bodies.find((b) => b.bodyId === 'body_a')?.name, 'Hex bolt')
  assert.ok(documentsAlign(documentForAgent(renameDraft.nextIrCode, aligned)!, twoBody))

  assert.equal(
    planSetBodyVisible({ irCode: aligned, lastGoodIrCode: aligned, bodyId: 'nope', visible: false }),
    null,
  )

  // Cycle 4 dirty-editor delete still stays editor-only (do not loosen).
  const dirtyDelete = planDeleteBody({
    irCode: dirtyIr,
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
  })
  assert.ok(dirtyDelete && dirtyDelete.kind === 'editor-only')
}

// 12. Invalid / empty editor: Outliner last-good mutate applies to the scene
{
  const twoBody = parseSceneJson(JSON.stringify({
    documentId: 'two',
    units: 'mm',
    bodies: [
      {
        bodyId: 'body_a',
        name: 'Bolt',
        visible: true,
        features: [{ op: 'box', size: [10, 10, 10] }],
      },
      {
        bodyId: 'body_b',
        name: 'Nut',
        visible: true,
        features: [{ op: 'cylinder', diameter: 8, height: 6 }],
      },
    ],
  }))
  const aligned = prettyDocument(twoBody)
  const invalid = '{ "bodies": [ }'
  const removed = removeBodyFromDocument(twoBody, 'body_a')
  assert.ok(removed)
  const hiddenDoc = setBodyVisibleInDocument(twoBody, 'body_a', false)
  assert.ok(hiddenDoc)
  const renamedDoc = renameBodyInDocument(twoBody, 'body_a', 'Hex bolt')
  assert.ok(renamedDoc)

  const invalidDelete = planDeleteBody({
    irCode: invalid,
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
  })
  assert.ok(invalidDelete && invalidDelete.kind === 'scene', 'invalid JSON applies to last-good')
  assert.equal(invalidDelete.label, 'Delete Bolt')
  assert.ok(documentsAlign(parseSceneJson(invalidDelete.nextIrCode), removed))
  // After the store replaces the unusable draft, chat / export last-good match.
  assert.ok(
    documentsAlign(documentForAgent(invalidDelete.nextIrCode, invalidDelete.nextIrCode)!, removed),
  )

  const emptyDelete = planDeleteBody({
    irCode: '',
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
  })
  assert.ok(emptyDelete && emptyDelete.kind === 'scene', 'empty editor applies to last-good')
  assert.ok(documentsAlign(parseSceneJson(emptyDelete.nextIrCode), removed))

  const invalidHide = planSetBodyVisible({
    irCode: invalid,
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
    visible: false,
  })
  assert.ok(invalidHide && invalidHide.kind === 'scene')
  assert.equal(invalidHide.label, 'Hide Bolt')
  assert.ok(documentsAlign(parseSceneJson(invalidHide.nextIrCode), hiddenDoc))
  assert.ok(
    documentsAlign(documentForAgent(invalidHide.nextIrCode, invalidHide.nextIrCode)!, hiddenDoc),
  )

  const emptyHide = planSetBodyVisible({
    irCode: '',
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
    visible: false,
  })
  assert.ok(emptyHide && emptyHide.kind === 'scene')
  assert.ok(documentsAlign(parseSceneJson(emptyHide.nextIrCode), hiddenDoc))

  const invalidRename = planRenameBody({
    irCode: invalid,
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
    name: 'Hex bolt',
  })
  assert.ok(invalidRename && invalidRename.kind === 'scene')
  assert.equal(invalidRename.label, 'Rename Bolt → Hex bolt')
  assert.ok(documentsAlign(parseSceneJson(invalidRename.nextIrCode), renamedDoc))

  const emptyRename = planRenameBody({
    irCode: '',
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
    name: 'Hex bolt',
  })
  assert.ok(emptyRename && emptyRename.kind === 'scene')
  assert.ok(documentsAlign(parseSceneJson(emptyRename.nextIrCode), renamedDoc))

  assert.equal(
    planDeleteBody({ irCode: invalid, lastGoodIrCode: '', bodyId: 'body_a' }),
    null,
    'invalid JSON with no last-good still no-ops',
  )
  assert.equal(
    planSetBodyVisible({ irCode: invalid, lastGoodIrCode: '', bodyId: 'body_a', visible: false }),
    null,
  )
  assert.equal(
    planRenameBody({ irCode: invalid, lastGoodIrCode: '', bodyId: 'body_a', name: 'Hex bolt' }),
    null,
  )
  assert.equal(
    planDeleteBody({ irCode: invalid, lastGoodIrCode: aligned, bodyId: 'nope' }),
    null,
  )

  // Dirty parseable editor still cannot touch last-good (Cycles 4–5).
  const dirtyIr = prettyDocument({
    ...twoBody,
    bodies: twoBody.bodies.map((b) =>
      b.bodyId === 'body_b' ? { ...b, name: 'Nut draft' } : b,
    ),
  })
  const dirtyDelete = planDeleteBody({
    irCode: dirtyIr,
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
  })
  assert.ok(dirtyDelete && dirtyDelete.kind === 'editor-only')
  assert.ok(documentsAlign(documentForAgent(dirtyDelete.nextIrCode, aligned)!, twoBody))
  const dirtyHide = planSetBodyVisible({
    irCode: dirtyIr,
    lastGoodIrCode: aligned,
    bodyId: 'body_a',
    visible: false,
  })
  assert.ok(dirtyHide && dirtyHide.kind === 'editor-only')
  assert.ok(documentsAlign(documentForAgent(dirtyHide.nextIrCode, aligned)!, twoBody))
}

// 13. History restore must drop isolate/select when the snapshot lacks that body
{
  const two = [{ bodyId: 'body_a' }, { bodyId: 'body_b' }]
  const one = [{ bodyId: 'body_a' }]

  assert.deepEqual(
    retainBodySelection(two, 'body_b', 'body_b'),
    { selectedBodyId: 'body_b', isolatedBodyId: 'body_b' },
    'keep isolate/select when the snapshot still has the body',
  )
  assert.deepEqual(
    retainBodySelection(one, 'body_b', 'body_b'),
    { selectedBodyId: null, isolatedBodyId: null },
    'orphan isolate would hide every mesh while chat/export still send the snapshot',
  )
  assert.deepEqual(
    retainBodySelection(one, 'body_a', 'body_b'),
    { selectedBodyId: 'body_a', isolatedBodyId: null },
  )
  assert.deepEqual(
    retainBodySelection([], 'body_a', 'body_a'),
    { selectedBodyId: null, isolatedBodyId: null },
  )
  assert.deepEqual(
    retainBodySelection(two, null, null),
    { selectedBodyId: null, isolatedBodyId: null },
  )

  const lastGood = parseSceneJson(JSON.stringify({
    documentId: 'two',
    units: 'mm',
    bodies: [
      { bodyId: 'body_a', name: 'Bolt', features: [{ op: 'box', size: [10, 10, 10] }] },
      { bodyId: 'body_b', name: 'Nut', features: [{ op: 'cylinder', diameter: 8, height: 6 }] },
    ],
  }))
  assert.equal(targetBodyIdForDocument(lastGood, 'body_b'), 'body_b')
  assert.equal(
    targetBodyIdForDocument(lastGood, 'draft_only'),
    undefined,
    'do not silently scope chat to a body the agent document does not have',
  )
  assert.equal(targetBodyIdForDocument(lastGood, null), undefined)
  assert.equal(targetBodyIdForDocument(null, 'body_b'), undefined)
}

console.log('document.test.ts: all assertions passed')
