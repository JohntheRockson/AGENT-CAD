import assert from 'node:assert/strict'
import {
  canDownloadExport,
  exportFileName,
  exportMenuCaption,
  EXPORT_KINDS,
  LAST_GOOD_REBUILD_FAILED_NOTE,
  sanitizeExportBase,
} from './saveFile.ts'

const LAST_GOOD = '{ "features": [] }'
const DIRTY = '{ "features": [{ "op": "box", "w": 1, "d": 1, "h": 1 }] }'

assert.equal(EXPORT_KINDS[0].id, 'stl', 'STL should be the happy-path default, not STEP')
assert.ok(EXPORT_KINDS.some((k) => k.id === 'step' && k.caution), 'STEP must stay available but cautioned')
assert.notEqual(EXPORT_KINDS[0].id, 'step')

assert.equal(sanitizeExportBase('m8_bolt_40mm'), 'm8_bolt_40mm')
assert.equal(sanitizeExportBase('  foo/bar baz  '), 'foo_bar_baz')
assert.equal(sanitizeExportBase(''), 'model')
assert.equal(sanitizeExportBase(undefined), 'model')
assert.equal(exportFileName('m8_bolt_40mm', 'stl'), 'm8_bolt_40mm.stl')
assert.equal(exportFileName('', 'step'), 'model.step')

// Cycle 3: last-good + parseable + matching editor may export even when runError is set.
assert.deepEqual(
  canDownloadExport({ runError: 'kernel failed', irCode: LAST_GOOD, lastGoodIrCode: LAST_GOOD }),
  { ok: true, note: LAST_GOOD_REBUILD_FAILED_NOTE },
)

// Unparseable last-good still cannot export through a rebuild error.
assert.deepEqual(
  canDownloadExport({ runError: 'kernel failed', irCode: '{}', lastGoodIrCode: '{}' }),
  { ok: false, reason: 'Cannot export while a rebuild error is set. Fix or rebuild first.' },
)

assert.equal(
  canDownloadExport({ runError: null, irCode: '', lastGoodIrCode: '' }).ok,
  false,
)
assert.equal(
  canDownloadExport({ runError: 'kernel failed', irCode: '', lastGoodIrCode: '' }).ok,
  false,
)

// Dirty editor IR that ≠ last-good stays blocked — with or without runError.
assert.equal(
  canDownloadExport({
    runError: null,
    irCode: DIRTY,
    lastGoodIrCode: LAST_GOOD,
  }).ok,
  false,
)
assert.deepEqual(
  canDownloadExport({
    runError: 'Calculate failed',
    irCode: DIRTY,
    lastGoodIrCode: LAST_GOOD,
  }),
  {
    ok: false,
    reason: 'Rebuild the model before exporting. Current IR does not match the last successful run.',
  },
)
assert.equal(
  canDownloadExport({
    runError: 'JSON parse error',
    irCode: '',
    lastGoodIrCode: LAST_GOOD,
  }).ok,
  false,
  'cleared / dirty editor cannot export last-good without a rebuild',
)

assert.deepEqual(
  canDownloadExport({
    runError: null,
    irCode: LAST_GOOD,
    lastGoodIrCode: LAST_GOOD,
  }),
  { ok: true },
)

assert.equal(
  exportMenuCaption(
    { ok: true, note: LAST_GOOD_REBUILD_FAILED_NOTE },
    0,
  ),
  LAST_GOOD_REBUILD_FAILED_NOTE,
)
assert.equal(
  exportMenuCaption(
    { ok: true, note: LAST_GOOD_REBUILD_FAILED_NOTE },
    2,
  ),
  '2 uncommitted parameter changes — exporting last good; rebuild failed',
)
assert.ok(
  exportMenuCaption({ ok: true }, 1).includes('export is the last calculated'),
)
assert.equal(
  exportMenuCaption({ ok: false, reason: 'blocked' }, 0),
  'blocked',
)

// Panel Calculate drafts do not mutate IR — export stays last-good when IR matches.
// Dirty / invalid editor JSON still fails the string last-good gate (do not loosen).

console.log('saveFile.test.ts: all assertions passed')
