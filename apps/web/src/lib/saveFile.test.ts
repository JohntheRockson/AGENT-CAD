import assert from 'node:assert/strict'
import { canDownloadExport, exportFileName, EXPORT_KINDS, sanitizeExportBase } from './saveFile.ts'

assert.equal(EXPORT_KINDS[0].id, 'stl', 'STL should be the happy-path default, not STEP')
assert.ok(EXPORT_KINDS.some((k) => k.id === 'step' && k.caution), 'STEP must stay available but cautioned')
assert.notEqual(EXPORT_KINDS[0].id, 'step')

assert.equal(sanitizeExportBase('m8_bolt_40mm'), 'm8_bolt_40mm')
assert.equal(sanitizeExportBase('  foo/bar baz  '), 'foo_bar_baz')
assert.equal(sanitizeExportBase(''), 'model')
assert.equal(sanitizeExportBase(undefined), 'model')
assert.equal(exportFileName('m8_bolt_40mm', 'stl'), 'm8_bolt_40mm.stl')
assert.equal(exportFileName('', 'step'), 'model.step')

assert.deepEqual(
  canDownloadExport({ runError: 'kernel failed', irCode: '{}', lastGoodIrCode: '{}' }),
  { ok: false, reason: 'Cannot export while a rebuild error is set. Fix or rebuild first.' },
)
assert.equal(
  canDownloadExport({ runError: null, irCode: '', lastGoodIrCode: '' }).ok,
  false,
)
assert.equal(
  canDownloadExport({
    runError: null,
    irCode: '{ "dirty": true }',
    lastGoodIrCode: '{ "good": true }',
  }).ok,
  false,
)
assert.deepEqual(
  canDownloadExport({
    runError: null,
    irCode: '{ "good": true }',
    lastGoodIrCode: '{ "good": true }',
  }),
  { ok: true },
)

console.log('saveFile.test.ts: all assertions passed')
